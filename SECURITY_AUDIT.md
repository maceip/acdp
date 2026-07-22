# ACDP Security Audit Report

**Date:** 2026-07-22
**Scope:** Sandbox policy enforcement, execution state management, LLM interceptor routing, code generation, plan interpreter, resource limits, session management, FFI boundaries, OIDC client, TLS/DNS configuration

---

## Finding 1: Tool-Specific Policy Overrides Bypass Trust-Level Runtime Restrictions

**File:** `acdp-sandbox/src/policy.rs` lines 212–235
**Severity:** High
**Attacker-controlled input:** `tool_policies` map (loaded from configuration / deserialized)

**Vulnerable code:**

```rust
pub fn is_runtime_allowed(
    &self,
    tool_id: &str,
    trust_level: TrustLevel,
    runtime: RuntimeType,
) -> bool {
    if runtime == RuntimeType::Process && !self.allow_process_runtime {
        return false;
    }
    if let Some(tool_policy) = self.tool_policies.get(tool_id) {
        if !tool_policy.allowed_runtimes.is_empty() {
            return tool_policy.allowed_runtimes.contains(&runtime);
        }
    }
    // Fall back to trust-level defaults
    ...
}
```

**Issue:** When a tool-specific `ToolPolicy` exists, `is_runtime_allowed` uses **only** the tool's `allowed_runtimes` list without cross-checking the `trust_level` parameter. A `ToolPolicy` for an `Untrusted` tool can explicitly list `RuntimeType::V8` in its `allowed_runtimes`, even though the global trust-level defaults restrict untrusted tools to WASM-only. The `trust_level` field inside `ToolPolicy` is stored but **never consulted** during runtime authorization decisions — it is entirely informational.

**Existing controls:** The `allow_process_runtime` global flag blocks `Process` runtime regardless of per-tool policy. However, a V8 escalation for an untrusted tool is not blocked.

**Attack path:** If an attacker can influence the `tool_policies` configuration (e.g., via a deserialized config file, API endpoint, or LLM-generated plan metadata), they can register a tool with `TrustLevel::Untrusted` but `allowed_runtimes: [V8]`, bypassing the trust-level sandbox restriction. Similarly, the `ToolPolicy.trust_level` field is never validated to match the caller-supplied `trust_level`, allowing inconsistent privilege assertions.

---

## Finding 2: Plan Interpreter Executes Attacker-Supplied Code Without Sandboxing the Plan Itself

**File:** `acdp-sandbox/src/service.rs` lines 175–400
**Severity:** High
**Attacker-controlled input:** `ExecutionPlan` (received via `execute_plan` / `plan_and_execute`)

**Vulnerable code:**

```rust
async fn interpret_plan(&self, plan: ExecutionPlan) -> Result<ExecutionStream> {
    // ...
    for node in plan.graph.nodes.iter() {
        match &node.kind {
            PlanNodeKind::RunPython { code, capability } => {
                // ...
                let mut request = ExecutionRequest::new(code.clone());
                // ...
                let stream = self.execute(request).await...;
            }
            PlanNodeKind::ReadFile { path, output_capability } => {
                let content = tokio::fs::read_to_string(path).await...;
            }
            PlanNodeKind::WriteFile { path, input_capability } => {
                tokio::fs::write(path, data).await...;
            }
```

**Issue:** The `interpret_plan` method iterates over plan nodes sequentially. While it checks that each node's capability is present in the plan's policy, the **capability tokens themselves are embedded in the plan** — meaning whoever constructs the plan controls both the operations and the authorization. The `ensure_capability` function only verifies that the plan's own `policy.capabilities` list contains a matching token, which is a self-referential check. An externally supplied `ExecutionPlan` can include arbitrary capability tokens granting itself any permission.

**Existing controls:** Network access and mounts are explicitly rejected. Capability sink checks exist but are tautological when the plan author controls the token list.

**Attack path:** An attacker who can submit an `ExecutionPlan` directly (bypassing the code generator) can include arbitrary `CapabilityToken` entries with `origin: Trusted` and any `allowed_sinks`, then reference those capabilities in `ReadFile`/`WriteFile`/`RunPython` nodes. The path traversal aspect is already noted as known; this finding focuses on the self-referential authorization model.

---

## Finding 3: LLM Code Generator Injects Trusted Capability Without Validating Request Policy

**File:** `acdp-sandbox/src/gencode.rs` lines 93–105
**Severity:** Medium
**Attacker-controlled input:** `CodeGenerationRequest` (including `request.policy`)

**Vulnerable code:**

```rust
let capability_id = CapabilityId("sandbox_exec".to_string());
if !policy.capabilities.iter().any(|cap| cap.id == capability_id) {
    policy.capabilities.push(CapabilityToken {
        id: capability_id.clone(),
        kind: CapabilityKind::SandboxExecution,
        origin: CapabilityOrigin::Trusted,
        allowed_sinks: vec![CapabilitySink::SandboxExecution],
    });
}
```

**Issue:** Beyond the known auto-grant of `Trusted` capability (noted as known), the check `!policy.capabilities.iter().any(|cap| cap.id == capability_id)` only verifies by ID, not by kind/origin. An attacker can pre-populate the request policy with a capability token having `id: "sandbox_exec"` but with **additional** `allowed_sinks` (e.g., `FileRead`, `FileWrite`, `Emit`). Since the code only checks for ID existence, it skips the push and uses the attacker's more permissive token. This allows the generated plan to be interpreted with broader permissions than intended.

**Existing controls:** None — there is no validation that existing capabilities in the incoming request are appropriately scoped.

**Attack path:** Submit a `CodeGenerationRequest` with `policy.capabilities` pre-populated with a `sandbox_exec` token that includes `FileRead` and `FileWrite` sinks. The code generator will not overwrite it, and the subsequent plan interpretation will accept file operations under that capability.

---

## Finding 4: LiteRT FFI `from_ffi` Trusts C-Reported Array Lengths Without Bounds Validation

**File:** `acdp-llm/src/litert_wrapper.rs` lines 197–240
**Severity:** Medium (requires corrupted or malicious C library)
**Attacker-controlled input:** LiteRT C library return values

**Vulnerable code:**

```rust
unsafe fn from_ffi(ffi: *const LiteRtLmBenchmarkInfoFFI) -> LlmResult<Self> {
    let info = &*ffi;
    let prefill_turns: Vec<TurnBenchmark> = if info.total_prefill_turns > 0 {
        std::slice::from_raw_parts(info.prefill_turns, info.total_prefill_turns as usize)
            .iter()
            .map(...)
            .collect()
    };
    let decode_turns: Vec<TurnBenchmark> = if info.total_decode_turns > 0 {
        std::slice::from_raw_parts(info.decode_turns, info.total_decode_turns as usize)
            .iter()
            .map(...)
            .collect()
    };
```

**Issue:** `std::slice::from_raw_parts` is called with `total_prefill_turns` and `total_decode_turns` as lengths. These values come from the C FFI struct and are **trusted without validation**. If the C library returns a `total_prefill_turns` value larger than the actual allocated array, this creates an out-of-bounds read. The `u32` to `usize` cast (`as usize`) is safe on all platforms, but the underlying pointer validity is not checked beyond the null check on `ffi` itself — the inner `prefill_turns` and `decode_turns` pointers are not null-checked before `from_raw_parts`.

**Existing controls:** The null check on `ffi` prevents dereferencing a null top-level pointer, but inner pointers `info.prefill_turns` and `info.decode_turns` may be null even when counts are > 0.

**Attack path:** If a malicious or corrupted model file causes the LiteRT C library to return inconsistent benchmark data (non-zero count with null or undersized pointer), this causes undefined behavior: out-of-bounds reads, potential information disclosure, or crashes.

---

## Finding 5: `unsafe impl Send + Sync` on FFI Wrapper Types Without Thread-Safety Guarantees

**File:** `acdp-llm/src/litert_wrapper.rs` lines 264–265, 407–408
**Severity:** Medium
**Attacker-controlled input:** Not directly; exploitable via concurrent usage patterns

**Vulnerable code:**

```rust
unsafe impl Send for LiteRTEngine {}
unsafe impl Sync for LiteRTEngine {}
// ...
unsafe impl Send for LiteRTConversation {}
unsafe impl Sync for LiteRTConversation {}
```

**Issue:** Both `LiteRTEngine` and `LiteRTConversation` are thin wrappers around raw C pointers (`*mut c_void`). They are marked `Send + Sync` with the comment "The underlying C++ conversation is thread-safe", but there is no evidence this claim is verified. If the C++ LiteRT-LM library is not truly thread-safe (many inference engines are not), concurrent use of `LiteRTConversation::send_message` from multiple async tasks could cause data races in the C library. The `LiteRTEngine` is wrapped in `Arc<RwLock<...>>` at the manager level, but `LiteRTConversation` instances can be shared via `Arc` without any Rust-level synchronization.

**Existing controls:** `ModelManager` wraps the engine in `Arc<RwLock<Option<Arc<LiteRTEngine>>>>`, providing some serialization at the engine level. Individual conversations are not protected.

**Attack path:** If multiple async tasks concurrently call `send_message` on the same `LiteRTConversation` (e.g., via the warm_session or parallel prediction paths), and the underlying C library is not thread-safe, this could cause heap corruption or use-after-free in the native code.

---

## Finding 6: Sandbox Execution Map Never Automatically Cleaned Up (Memory Leak / DoS)

**File:** `acdp-sandbox/src/service.rs` lines 57–77, 114–129
**Severity:** Medium
**Attacker-controlled input:** Execution requests

**Vulnerable code:**

```rust
pub async fn execute_with_id(&self, id: ExecutionId, request: ExecutionRequest) -> Result<ExecutionStream> {
    let state = Arc::new(RwLock::new(ExecutionState::new(id, request.clone())));
    self.executions.write().await.insert(id, state.clone());
    let stream = self.runtime.execute(request).await?;
    Ok(stream)
}
```

**Issue:** Every call to `execute_with_id` inserts an entry into the `executions` HashMap, but `cleanup_completed` is never called automatically — it is a public method that must be invoked externally. There is no TTL, no size limit, and no background cleanup task. An attacker who can submit execution requests can grow this map unboundedly, consuming memory until the service OOMs.

**Existing controls:** `cleanup_completed` exists but is opt-in. No periodic cleanup or capacity limit.

**Attack path:** Submit a large number of execution requests. Each one is stored in the map permanently (even after completion) until an external caller invokes `cleanup_completed`. The `ExecutionState` includes buffered stdout/stderr (up to 1000 lines each), amplifying memory consumption.

---

## Finding 7: Execution Cleanup Race Condition via `try_read`

**File:** `acdp-sandbox/src/service.rs` lines 114–129
**Severity:** Low
**Attacker-controlled input:** Concurrent execution submissions

**Vulnerable code:**

```rust
pub async fn cleanup_completed(&self) {
    let mut executions = self.executions.write().await;
    executions.retain(|_, state| {
        let status = state.try_read().map(|s| s.status.clone());
        if let Ok(status) = status {
            !matches!(status, ExecutionStatus::Completed | ExecutionStatus::Failed | ExecutionStatus::Cancelled)
        } else {
            true // Keep if we can't read (lock contention)
        }
    });
}
```

**Issue:** `try_read()` is used inside `retain`, meaning if any execution's `RwLock` is write-locked at cleanup time (e.g., by a monitoring task writing status), that execution is retained regardless of its actual status. An attacker could keep executions perpetually locked (or time them to contend with cleanup), causing completed executions to never be removed.

**Existing controls:** The outer `executions` write lock ensures no new entries are inserted during cleanup, but individual state locks can still contend.

**Attack path:** Sustain concurrent writes to execution states (e.g., via stdout/stderr push operations) timed to overlap with cleanup invocations, preventing garbage collection.

---

## Finding 8: LLM Interceptor Can Be Disabled at Runtime Without Authentication

**File:** `acdp-llm/src/interceptor.rs` lines 100–141
**Severity:** Medium
**Attacker-controlled input:** Routing mode changes via IPC

**Vulnerable code:**

```rust
pub async fn set_routing_mode(&self, mode: RoutingMode) {
    let mut guard = self.routing_mode.write().await;
    *guard = mode;
}

pub async fn add_interceptor_at_runtime(&self, interceptor: Arc<dyn MessageInterceptor>) -> LlmResult<()> {
    if let Some(ref manager) = self.interceptor_manager {
        manager.add_interceptor(interceptor).await;
        Ok(())
    } else { ... }
}

pub async fn remove_interceptor_at_runtime(&self, name: &str) -> LlmResult<bool> {
    if let Some(ref manager) = self.interceptor_manager {
        Ok(manager.remove_interceptor(name).await)
    } else { ... }
}
```

**Issue:** `set_routing_mode`, `add_interceptor_at_runtime`, and `remove_interceptor_at_runtime` have no authorization checks. The comment "callable by LLM" on add/remove suggests these are intended for programmatic use, but they can be invoked by anyone with access to the `LlmInterceptor` instance. Setting routing mode to `Bypass` disables all LLM-based security routing. Removing interceptors by name is unauthenticated.

**Existing controls:** Access requires holding a reference to the `LlmInterceptor`, which limits the attack surface to code paths that expose it (IPC commands, CLI). The IPC socket path is known (related to the known finding about predictable `/tmp` path).

**Attack path:** Via the IPC socket, send a `SetRoutingMode { mode: "bypass" }` command or remove security-critical interceptors by name, disabling the entire routing/interception pipeline.

---

## Finding 9: Pending Predictions Map Unbounded Growth

**File:** `acdp-llm/src/interceptor.rs` lines 40, 187–198
**Severity:** Low
**Attacker-controlled input:** Outgoing MCP requests

**Vulnerable code:**

```rust
pending_predictions: Arc<Mutex<HashMap<String, PendingPrediction>>>,
// ...
if let Some(request_id) = self.extract_request_id(&message) {
    // ...
    let mut map = self.pending_predictions.lock().await;
    map.insert(request_id, pending);
}
```

**Issue:** Every outgoing request with a parseable request ID inserts into `pending_predictions`. Entries are only removed when a matching response arrives (in `update_prediction_accuracy`). If responses are lost, delayed, or never arrive (e.g., one-shot notifications that never get responses, or backend timeouts), entries accumulate without bound. There is no TTL or size limit.

**Existing controls:** None.

**Attack path:** Send a high volume of requests that will never receive responses (or use request IDs that don't match any future response), causing the HashMap to grow indefinitely and consume memory.

---

## Finding 10: Rauthy `verify_id_token` Always Fails — Authentication Bypass via Fallback

**File:** `acdp-gateway/acdp-server/src/services/rauthy_client.rs` lines 25–35
**Severity:** High
**Attacker-controlled input:** ID tokens

**Vulnerable code:**

```rust
pub async fn verify_id_token(&self, id_token: &str) -> Result<IDTokenClaims> {
    // TODO: Implement proper Rauthy token introspection
    Err(ACDPGatewayError::RauthyError(
        "ID token verification not yet implemented".to_string(),
    ))
}
```

**Issue:** `verify_id_token` is a stub that **always returns an error**. This means any code path that relies on ID token verification will either: (a) fail closed and deny all access (safe but broken), or (b) catch the error and fall through to a permissive default (auth bypass). Combined with the known `validate_client` always-true issue, the entire Rauthy client provides no real authentication. The `admin_token` field is stored but never used in any request (the TODO references `POST /oidc/introspect with admin token` but it's never implemented).

**Existing controls:** The `get_user_info` endpoint works correctly, but it requires a valid `access_token` which this client cannot verify.

**Attack path:** Depends on how callers handle the error. If any caller treats a `verify_id_token` failure as a soft error and proceeds without verification, an attacker can supply any JWT (or no JWT) and bypass authentication. The unused `admin_token` also means the introspection endpoint is never called, so token revocation checks are impossible.

---

## Finding 11: DNS Credentials Stored in Plaintext Configuration

**File:** `acdp-llm/src/config.rs` lines 289–291
**Severity:** Medium
**Attacker-controlled input:** Configuration file

**Vulnerable code:**

```rust
/// DNS provider credentials (JSON string or path to file)
#[serde(default)]
pub dns_credentials: Option<String>,
```

**Issue:** DNS provider credentials (e.g., Cloudflare API tokens, AWS Route53 keys) for ACME DNS-01 challenges are stored as a plaintext string in the TOML configuration file. The field accepts either a JSON string of credentials or a path to a file. When serialized (via `AppConfig::save()`), these credentials are written to `~/.config/assist-mcp/config.toml` in plaintext. The `Serialize` derive on `TlsConfig` means credentials will be included in any serialization (logging, debug output, config export).

**Existing controls:** None — no encryption, no permission checks on the config file, no masking in `Debug` derive.

**Attack path:** Read `~/.config/assist-mcp/config.toml` to obtain DNS provider API credentials, which can then be used to modify DNS records for the configured domains (enabling domain takeover, certificate issuance for arbitrary subdomains, etc.).

---

## Finding 12: Model Download Path Traversal via Crafted Model Name

**File:** `acdp-llm/src/model_management.rs` lines 365–366
**Severity:** Medium
**Attacker-controlled input:** `model_name` parameter

**Vulnerable code:**

```rust
let safe_name = model_name.replace('/', "_");
let model_path = cache_dir.join(&safe_name);
```

**Issue:** The sanitization only replaces `/` with `_`. On Unix systems, a model name containing `..` (without slashes) won't be traversed because `cache_dir.join("..something")` resolves within the cache directory. However, model names containing null bytes, control characters, or platform-specific path separators (Windows `\`) are not filtered. More critically, the `temp_path = model_path.with_extension("tmp")` pattern means a model name like `important_file.toml` would create `important_file.tmp` in the cache directory, and after download, rename it to `important_file.toml` — potentially overwriting legitimate files in the cache directory.

Additionally, the download URL is constructed directly from the model name without validation:
```rust
let download_url = if model_name.starts_with("google/") {
    format!("https://www.kaggle.com/models/{}/download", model_name)
} else {
    format!("https://huggingface.co/{}/resolve/main/model.gguf", model_name)
};
```
An attacker-supplied model name like `../../etc/passwd` becomes `https://huggingface.co/../../etc/passwd/resolve/main/model.gguf`, which could be manipulated for SSRF depending on the HTTP client's URL resolution.

**Existing controls:** The `/` to `_` replacement prevents basic directory traversal in the local path.

**Attack path:** Supply a crafted model name via the settings panel or API that either (1) overwrites files within the cache directory, or (2) causes the HTTP client to resolve to an unintended URL.

---

## Finding 13: Session Manager Never Expires Inactive Sessions Automatically

**File:** `acdp-llm/src/session_management.rs` lines 157–316
**Severity:** Low
**Attacker-controlled input:** Session creation via MCP requests

**Vulnerable code:**

```rust
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<SessionId, SessionPredictionContext>>>,
    // ...
}

pub async fn cleanup_inactive_sessions(&self, max_age_hours: i64) {
    let mut sessions = self.sessions.write().await;
    let cutoff = Utc::now() - chrono::Duration::hours(max_age_hours);
    sessions.retain(|_id, context| context.last_updated > cutoff);
}
```

**Issue:** While `cleanup_inactive_sessions` exists, it is never called by any background task or periodic timer. Sessions accumulate indefinitely in the `sessions` HashMap. Each `SessionPredictionContext` stores unbounded `message_history` and `predictions` vectors, so long-lived sessions can consume significant memory.

**Existing controls:** `cleanup_inactive_sessions` exists as a manual method but has no automatic invocation.

**Attack path:** Create many sessions (each MCP connection generates a session) and keep them active with periodic requests. The message history and prediction data grow without bound, eventually exhausting available memory.

---

## Finding 14: Stderr Redirect Creates World-Readable Log File in CWD

**File:** `acdp-tui/src/lib.rs` lines 52–66
**Severity:** Low
**Attacker-controlled input:** Not directly

**Vulnerable code:**

```rust
let _stderr_guard = unsafe {
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("litert.log")
        .ok();

    if let Some(file) = log_file {
        let original_stderr = libc::dup(libc::STDERR_FILENO);
        libc::dup2(file.as_raw_fd(), libc::STDERR_FILENO);
        Some((original_stderr, file))
    } else {
        None
    }
};
```

**Issue:** The LiteRT log file is created at `litert.log` in the current working directory with default permissions (typically 0644 on Linux). Any stderr output (including potential error messages containing sensitive data like file paths, memory addresses, or partial credentials) is written to this world-readable file. The file creation uses `OpenOptions::new().create(true).append(true)`, which does not set restrictive permissions. Additionally, if the CWD is a shared/writable directory, another user could pre-create `litert.log` as a symlink to an arbitrary file, causing stderr to be appended to that target.

**Existing controls:** None.

**Attack path:** Read `litert.log` for information disclosure, or pre-create it as a symlink for log injection into arbitrary files.

---

## Finding 15: `run_monitor_app` Uses `set_var` Which Is Unsound in Multi-Threaded Context

**File:** `acdp-tui/src/lib.rs` lines 118–124
**Severity:** Low
**Attacker-controlled input:** `MonitorArgs.ipc_socket`, `MonitorArgs.verbose`

**Vulnerable code:**

```rust
pub async fn run_monitor_app(args: MonitorArgs) -> Result<()> {
    std::env::set_var("MCP_IPC_SOCKET", &args.ipc_socket);
    if args.verbose {
        std::env::set_var("RUST_LOG", "debug");
    }
    launch_tui().await
}
```

**Issue:** `std::env::set_var` is documented as unsound when called in a multi-threaded program (it is `unsafe` since Rust 1.66 in the 2024 edition). In an async context (this is called from within a Tokio runtime), multiple threads may be reading environment variables concurrently. This is undefined behavior per POSIX `setenv`/`getenv` thread-safety guarantees.

**Existing controls:** None.

**Attack path:** Not directly exploitable by an external attacker, but concurrent environment reads during `set_var` could cause memory corruption or crashes in multi-threaded Tokio runtimes. This is a correctness/soundness issue.

---

## Finding 16: HTTP Session Hijacking via Predictable UUID Session IDs

**File:** `acdp-tui/src/http_server.rs` lines 71–75, 312–319
**Severity:** Low
**Attacker-controlled input:** `Mcp-Session-Id` header

**Vulnerable code:**

```rust
let session_id = headers
    .get("Mcp-Session-Id")
    .and_then(|h| h.to_str().ok())
    .map(|s| s.to_string())
    .unwrap_or_else(|| Uuid::new_v4().to_string());
```

```rust
async fn handle_sse_stream(
    State(state): State<HttpServerState>,
    Path(session_id): Path<String>,
) -> Result<Sse<...>, StatusCode> {
    let sessions = state.sessions.lock().await;
    let session = sessions.get(&session_id).ok_or(StatusCode::NOT_FOUND)?;
    let mut rx = session.response_tx.subscribe();
```

**Issue:** Session IDs are either client-provided (via the `Mcp-Session-Id` header) or auto-generated UUIDs. A client can supply **any string** as a session ID. If two clients supply the same session ID, the second client reuses the first client's session (including its backend connection and interceptor pipeline). The SSE endpoint (`/sse/:session_id`) similarly allows any client to subscribe to another session's response stream by knowing/guessing its session ID.

**Existing controls:** None — no session ownership verification, no authentication on SSE endpoint.

**Attack path:** (1) Supply a known session ID in the `Mcp-Session-Id` header to hijack another client's session. (2) Subscribe to `/sse/<session_id>` to eavesdrop on another session's responses. UUIDv4 is random enough to prevent brute-force, but client-supplied IDs can be pre-shared or leaked.

---

## Summary Table

| # | Finding | Severity | File | Category |
|---|---------|----------|------|----------|
| 1 | Tool policy overrides bypass trust-level restrictions | High | `policy.rs:212-235` | Authorization |
| 2 | Plan interpreter self-referential capability auth | High | `service.rs:175-400` | Authorization |
| 3 | Pre-populated capability ID bypasses sink restrictions | Medium | `gencode.rs:93-105` | Authorization |
| 4 | FFI `from_ffi` trusts C array lengths without validation | Medium | `litert_wrapper.rs:197-240` | Memory Safety |
| 5 | `unsafe impl Send+Sync` on unverified FFI types | Medium | `litert_wrapper.rs:264-265,407-408` | Memory Safety |
| 6 | Execution map grows unbounded (no cleanup) | Medium | `service.rs:57-77` | DoS |
| 7 | Cleanup race condition via `try_read` | Low | `service.rs:114-129` | Race Condition |
| 8 | Interceptor pipeline can be disabled without auth | Medium | `interceptor.rs:100-141` | Authorization |
| 9 | Pending predictions map unbounded growth | Low | `interceptor.rs:40,187-198` | DoS |
| 10 | `verify_id_token` stub — auth bypass via error fallback | High | `rauthy_client.rs:25-35` | Authentication |
| 11 | DNS credentials in plaintext config | Medium | `config.rs:289-291` | Secrets Management |
| 12 | Model download partial path traversal / SSRF | Medium | `model_management.rs:365-366` | Input Validation |
| 13 | Session manager never auto-cleans sessions | Low | `session_management.rs:157-316` | DoS |
| 14 | Stderr redirect creates world-readable log | Low | `lib.rs:52-66` | Information Disclosure |
| 15 | `set_var` unsound in async/multi-threaded context | Low | `lib.rs:118-124` | Memory Safety |
| 16 | HTTP session hijacking via client-supplied IDs | Low | `http_server.rs:71-75` | Session Management |
