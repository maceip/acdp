# ACDP Security Audit Report — SQL Injection & Command Injection

**Auditor:** Automated Security Scanner  
**Date:** 2026-07-02  
**Scope:** SQL injection, command injection, and code execution vulnerabilities  
**Excludes:** Path traversal (sandbox service.rs), SSRF (V8 worker.rs), LLM auto-grant Trusted capability, prompt injection (DSPy predictor) — already known

---

## Executive Summary

The codebase demonstrates generally good SQL hygiene across both the PostgreSQL gateway (`sqlx::query!` compile-time macro) and the SQLite LLM service (`sqlx::query` / `sqlx::query_as` with `.bind()` parameters). **No SQL injection vulnerabilities were found.**

However, **three validated command injection / arbitrary code execution findings** were identified, all in production (non-test) code paths where attacker-influenced input can reach OS shell invocation.

| # | Severity | Category | Location |
|---|----------|----------|----------|
| 1 | **CRITICAL** | Command Injection via Shell | `acdp-sandbox/src/runtime/process.rs:50-52` |
| 2 | **HIGH** | Command Injection via Shell | `acdp-transport/src/backend_connection.rs:59-62` |
| 3 | **HIGH** | Command Injection via Shell | `acdp-transport/src/proxy.rs:608-616` |

---

## Finding 1 — CRITICAL: Arbitrary OS Command Execution in ProcessRuntime

**File:** `acdp-sandbox/src/runtime/process.rs`, lines 50-52  
**Category:** Command Injection (CWE-78)  
**Severity:** CRITICAL

### Vulnerable Code

```rust
let mut child = match Command::new(&shell)
    .arg("-c")
    .arg(&request.code)   // <-- attacker-controlled code string
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .stdin(std::process::Stdio::null())
    .spawn()
```

### Attack Path

1. **Entry point:** `ExecutionRequest.code` field (defined in `acdp-sandbox/src/types.rs:10` as `pub code: String`) carries the code to execute.
2. **Flow:** `SandboxService::execute()` → `ProcessRuntime::execute()` → `Command::new(shell).arg("-c").arg(&request.code)`
3. **Multiple ingestion paths:**
   - Direct via `SandboxService::execute(request)` (e.g., from MCP tool invocations)
   - Via `SandboxService::execute_plan()` → `interpret_plan()` which handles `PlanNodeKind::RunPython { code, .. }` (line 224 of `service.rs`) — the `code` field is extracted from an `ExecutionPlan` graph node and passed directly to `ExecutionRequest::new(code.clone())`
   - Via `SandboxService::plan_and_execute()` which calls the LLM-backed `LlmCodeGenerator` to produce code (from `acdp-sandbox/src/gencode.rs:81-84`) and immediately executes it

4. **Impact:** Full OS command execution with the privileges of the sandbox process. The `shell` defaults to `$SHELL` or `/bin/sh`. The `request.code` is passed as a single `-c` argument to the shell, so any shell metacharacters (`; && || | $(...)` etc.) are interpreted.

### Why This Is Critical

The `ProcessRuntime` is one of three available runtimes (`process`, `wasm`, `v8`). When selected (either explicitly or via runtime auto-selection in `acdp-sandbox/src/selector.rs`), user-supplied or LLM-generated code is passed directly to `/bin/sh -c`. While the `WasmRuntime` provides proper sandboxing, the `ProcessRuntime` provides **no sandboxing whatsoever** — there is no seccomp, no namespace isolation, no chroot, and no input sanitization.

The `LlmCodeGenerator` (`gencode.rs:59-132`) is particularly dangerous: it takes user-supplied `CodeGenerationSpec.description` and `context`, sends them to an LLM, extracts raw code from the response, and creates an `ExecutionPlan` with `CapabilityOrigin::Trusted` auto-granted capability (line 102). This means LLM-generated code bypasses capability checks entirely.

### Recommendation

- Default to WasmRuntime; require explicit opt-in for ProcessRuntime with security warnings
- If ProcessRuntime must exist, apply OS-level sandboxing (seccomp-bpf, namespaces, cgroups)
- Never auto-grant `CapabilityOrigin::Trusted` for LLM-generated code
- Validate execution plan nodes against an allowlist before execution

---

## Finding 2 — HIGH: Shell Command Injection in ProcessBackendConnection

**File:** `acdp-transport/src/backend_connection.rs`, lines 59-66  
**Category:** Command Injection (CWE-78)  
**Severity:** HIGH

### Vulnerable Code

```rust
pub async fn spawn(command: &str) -> Result<Self> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)       // <-- passes command string directly to shell
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
```

### Attack Path

1. **Entry point:** The `command` parameter comes from `backend_command` configuration.
2. **Flow:** `create_backend_connection(backend_url=None, backend_command=Some(cmd), ...)` → `ProcessBackendConnection::spawn(cmd)` → `Command::new("sh").arg("-c").arg(command)`
3. **Configuration sources (traced from `acdp-tui/src/config.rs:70-71`):**
   - Environment variable `MCP_BACKEND_COMMAND`
   - Config file field `mcp_server.backend_command`
4. **Invocation path:** When a new MCP client connects to the TUI server (`acdp-tui/src/acdp_server.rs:345-348`), the `backend_command` is read from `AppStateInner` and passed to `ClientProxy::new()` → `create_backend_connection()` → `ProcessBackendConnection::spawn()`

### Impact

The command string is passed directly to `sh -c` without any sanitization. If the `MCP_BACKEND_COMMAND` environment variable or config file is writable by an attacker, or if a malicious config is loaded, arbitrary commands execute with the server's privileges.

**Mitigating factor:** The command originates from server-side configuration (environment variable or config file), not directly from network input. However, if configuration is populated from a shared or user-writable source, this becomes exploitable.

### Recommendation

- Parse the command using `shell-words` or similar to extract argv, then use `Command::new(argv[0]).args(&argv[1..])` instead of `sh -c`
- Validate the backend command against an allowlist of permitted executables
- Log all backend command invocations for audit

---

## Finding 3 — HIGH: Shell Command Injection in MCP Proxy Server Startup

**File:** `acdp-transport/src/proxy.rs`, lines 608-616  
**Category:** Command Injection (CWE-78)  
**Severity:** HIGH

### Vulnerable Code

```rust
let child = if *use_shell {
    // Use shell to execute the command
    Command::new("sh")
        .arg("-c")
        .arg(command)       // <-- command from transport config
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?
} else {
    // Parse command and arguments
    let parts: Vec<&str> = command.split_whitespace().collect();
    // ... Command::new(parts[0]).args(&parts[1..])
```

### Attack Path

1. **Entry point:** `OutboundTransport::Stdio { command, use_shell }` in the transport configuration.
2. **Flow:** `MCPProxy::start()` → `start_mcp_server()` → `Command::new("sh").arg("-c").arg(command)`
3. **Configuration sources:**
   - CLI argument `--command` in `acdp-transport/src/main.rs:16-17`
   - Programmatic construction in `acdp-tui/src/app.rs:690` where it uses `format!("python3 {}", test_server_path.to_string_lossy())`
   - `TransportConfig::from_cli_args()` in `transport_config.rs:62-96`

### Impact

When `use_shell` is true (the default — see `acdp-transport/src/main.rs:31`), the entire command string is passed to `sh -c`. The `--command` flag is a single string argument that will be shell-interpreted.

The non-shell path (lines 619-627) uses a naive `split_whitespace()` parser, which does not handle quoting, escaping, or other shell constructs — this can lead to argument injection if the command string contains spaces in paths.

**Mitigating factor:** The command comes from CLI arguments at startup, not from runtime network input. However, if the proxy is spawned programmatically with user-derived command strings (as in the TUI at line 690), the risk increases.

### Recommendation

- Use `shell-words::split()` for proper POSIX-style argument parsing instead of `split_whitespace()`
- Consider removing `use_shell: true` as the default
- Validate the command executable against an allowlist before spawning

---

## SQL Injection Analysis — NO FINDINGS

### acdp-gateway (PostgreSQL via sqlx)

All SQL queries in the gateway use the **`sqlx::query!` compile-time macro** with numbered bind parameters (`$1`, `$2`, etc.):

- `acdp-gateway/acdp-server/src/services/credential.rs:333-354` — INSERT with `$1`-`$11`
- `acdp-gateway/acdp-server/src/routes/credential_verify.rs:37-46` — SELECT with `$1`
- `acdp-gateway/acdp-server/src/routes/credential_verify.rs:75-84` — UPDATE with `$1`

The `sqlx::query!` macro validates queries against the database schema at compile time, making SQL injection impossible in these paths.

### acdp-llm (SQLite via sqlx)

All SQL queries in the LLM module use **`sqlx::query` / `sqlx::query_as` with `.bind()` parameterization**:

- `acdp-llm/src/database/routing_rules.rs` — 5 queries, all use `?` placeholders with `.bind()`
- `acdp-llm/src/database/predictions.rs` — 6 queries, all use `?` placeholders with `.bind()`
- `acdp-llm/src/database/metrics.rs` — 4 queries, all use `?` placeholders with `.bind()`
- `acdp-llm/src/database/gepa.rs` — 5 queries, all use `?` placeholders with `.bind()`

**No `format!()`, string concatenation, or string interpolation was found in any SQL query string.** The `format!("sqlite://{}", ...)` in `acdp-llm/src/service.rs:52` constructs a connection URL from a config-derived file path, not a query — this is not SQL injection.

### Raw/Unchecked Query Patterns

A search for `query_unchecked`, `raw_sql`, `execute_raw`, and `raw_query` found **zero matches** across the entire codebase.

---

## Additional Observations

### V8 Worker `op_fetch` — SSRF (Already Known, Not Reported)
`acdp-sandbox/src/runtime/v8/worker.rs:12-30` — The `op_fetch` Deno operation allows JavaScript code running in V8 to fetch arbitrary URLs via `reqwest::get(&url)` with no URL validation or allowlist.

### LLM Code Generator Auto-Grants Trusted Capability (Already Known, Not Reported)
`acdp-sandbox/src/gencode.rs:99-104` — `CapabilityOrigin::Trusted` is auto-granted for LLM-generated plans, bypassing the capability-based security model.

---

## Summary of Recommendations

1. **Sandbox the ProcessRuntime** — Apply OS-level isolation (namespaces, seccomp) or deprecate in favor of WasmRuntime
2. **Eliminate `sh -c` patterns** — Use proper argument parsing (`shell-words` crate) instead of passing command strings to a shell
3. **Default `use_shell` to false** — The current default of `true` is dangerous
4. **Validate command executables** — Allowlist permitted binaries for backend commands
5. **Audit configuration sources** — Ensure `MCP_BACKEND_COMMAND` and config files cannot be influenced by untrusted users
6. **Remove `CapabilityOrigin::Trusted` auto-grant** — Require explicit user approval for LLM-generated code execution capabilities
