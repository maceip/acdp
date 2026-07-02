# ACDP Security Audit Report

**Date:** 2026-07-02  
**Scope:** Full codebase review of acdp-sandbox, acdp-transport, acdp-core, acdp-common  
**Auditor:** Automated security review

---

## Executive Summary

This audit identified **13 new vulnerabilities** across the ACDP codebase, ranging from critical sandbox escape vectors to moderate denial-of-service concerns. The most severe findings relate to the WASM runtime's WASI configuration, the process runtime inheriting the host environment, and missing request body size limits on all HTTP endpoints.

---

## Finding 1: WASM Runtime Inherits Host Process Arguments (HIGH)

**File:** `acdp-sandbox/src/runtime/wasm.rs`, line 108  
**Category:** Sandbox Escape / Information Disclosure

The WASI context builder calls `inherit_args()`, which passes the host process's command-line arguments into the sandboxed WASM module. This leaks potentially sensitive information (config paths, credentials passed as CLI args, internal flags) to untrusted WASM code.

```rust
let mut builder = WasiCtxBuilder::new();
builder.stdout(stdout_pipe);
builder.stderr(stderr_pipe);
let _ = builder.inherit_args(); // Ignore error if args can't be inherited
let wasi_ctx = builder.build_p1();
```

**Impact:** An untrusted WASM module can read the host process's argv, which may contain secrets, file paths, or internal configuration.

**Recommendation:** Remove `inherit_args()`. If arguments need to be passed to WASM modules, provide an explicit, sanitized argument list via `builder.args(&[...])`.

---

## Finding 2: WASM Resource Limits Are Not Actually Applied (HIGH)

**File:** `acdp-sandbox/src/runtime/wasm.rs`, lines 46-56  
**Category:** Denial of Service / Resource Exhaustion

When `with_limits()` is called, the code computes `max_pages` but never uses it. The `config.max_wasm_stack()` call only limits the native stack, not the WASM linear memory. The comment at line 114 confirms: "Store-level limits require implementing ResourceLimiter trait... For now, relying on engine-level limits." But the engine-level configuration doesn't actually enforce memory page limits.

```rust
if let Some(max_memory) = limits.max_memory_bytes {
    let max_pages = (max_memory / 65536) as u64;  // computed but NEVER USED
    config.max_wasm_stack(max_memory);              // only limits native stack
    config.memory_init_cow(false);
}
```

**Impact:** A malicious WASM module can allocate unlimited linear memory, causing host OOM.

**Recommendation:** Implement the `ResourceLimiter` trait on `StoreState` and attach it to the `Store` via `store.limiter(...)`. Use `max_pages` to limit `memory_growing()`.

---

## Finding 3: Process Runtime Inherits Full Host Environment (HIGH)

**File:** `acdp-sandbox/src/runtime/process.rs`, lines 50-56  
**Category:** Information Disclosure / Credential Leak

The `ProcessRuntime` spawns child processes without clearing or filtering environment variables. The child process inherits the full environment of the ACDP host, including any secrets, API keys, cloud credentials, or tokens present.

```rust
let mut child = match Command::new(&shell)
    .arg("-c")
    .arg(&request.code)
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .stdin(std::process::Stdio::null())
    .spawn()
```

Additionally, the shell itself is determined from the `SHELL` environment variable (line 18), which an attacker with control over the environment could manipulate.

**Impact:** Any code executed via the process runtime can read all environment variables, including cloud credentials (`AWS_SECRET_ACCESS_KEY`, `GOOGLE_APPLICATION_CREDENTIALS`, etc.), database passwords, and API tokens.

**Recommendation:** Use `Command::env_clear()` before spawning, then explicitly add only safe, necessary variables. Do not derive the shell from `$SHELL`; hardcode `/bin/sh`.

---

## Finding 4: Process Runtime Has No Working Directory Isolation (MEDIUM)

**File:** `acdp-sandbox/src/runtime/process.rs`, lines 50-56  
**Category:** Sandbox Escape

The spawned process inherits the host's current working directory. Combined with the known path traversal issue, this means executed code runs with full filesystem visibility from the CWD.

**Recommendation:** Set an explicit, restricted working directory via `Command::current_dir()`, ideally a temporary directory created per execution.

---

## Finding 5: No HTTP Request Body Size Limits (HIGH)

**Files:**
- `acdp-transport/src/http_sse_server.rs`, line 92 — `Json(body): Json<Value>`
- `acdp-transport/src/http_stream_server.rs`, line 74 — `Json(body): Json<Value>`

**Category:** Denial of Service

Both HTTP server endpoints use `axum::Json<Value>` to deserialize request bodies with no size limits configured. Axum's default body limit is 2MB, but `serde_json::Value` will attempt to parse arbitrarily deeply nested JSON. An attacker can send a deeply nested JSON document (e.g., `{"a":{"a":{"a":...}}}` to 1000+ levels) to exhaust stack space, or a large flat document to consume memory.

No `tower::limit::RequestBodyLimitLayer` or axum `.layer(DefaultBodyLimit::max(...))` is configured anywhere in the router setup.

```rust
pub fn create_router(state: HttpSseServerState) -> Router {
    Router::new()
        .route("/sse", post(handle_sse_post))
        .route("/sse", get(handle_sse_get))
        .route("/sse/:session_id", get(handle_sse_session))
        .with_state(state)
    // No body size limit layer
}
```

**Impact:** Memory exhaustion or stack overflow from crafted payloads.

**Recommendation:** Add `axum::extract::DefaultBodyLimit::max(1_048_576)` (1MB) as a layer on both routers. Consider also limiting JSON nesting depth if `serde_json` supports it, or use a streaming parser with limits.

---

## Finding 6: SSE Session Map Has No Limit — Connection Exhaustion (MEDIUM)

**File:** `acdp-transport/src/http_sse_server.rs`, lines 209-221  
**Category:** Denial of Service / Resource Exhaustion

The SSE GET handler creates a new session for every request that provides a new or missing `Mcp-Session-Id` header. Sessions are stored in a `HashMap` with no maximum size, no expiration, and no cleanup mechanism.

```rust
let mut sessions = state.sessions.lock().await;
if let Some(session) = sessions.get(&session_id) {
    session.sse_tx.subscribe()
} else {
    let (tx, rx) = broadcast::channel(100);
    let session = HttpSession { sse_tx: tx.clone() };
    sessions.insert(session_id.clone(), session);
    rx
}
```

**Impact:** An attacker can open thousands of SSE connections, each creating a new session entry with a `broadcast::channel(100)` allocation. Sessions are never cleaned up, leading to unbounded memory growth.

**Recommendation:** Add a maximum session count. Implement session TTL with a background cleanup task. Consider requiring authentication before session creation.

---

## Finding 7: SSE Event Injection via Unvalidated Session ID (LOW)

**File:** `acdp-transport/src/http_sse_server.rs`, lines 226-228  
**Category:** Injection

The session ID from the header is interpolated directly into SSE event data without sanitization:

```rust
yield Ok(Event::default()
    .event("connected")
    .data(format!(r#"{{"session_id":"{}"}}"#, session_id)));
```

While SSE data encoding by axum handles newlines correctly, the session ID is injected directly into a JSON string without JSON-escaping. A session ID containing `"` or `\` characters would produce malformed JSON that could confuse clients.

**Recommendation:** Use `serde_json::json!` to construct the event data instead of `format!`, ensuring proper JSON escaping.

---

## Finding 8: `#[serde(untagged)]` on `JsonRpcMessage` — Type Confusion (MEDIUM)

**File:** `acdp-core/src/messages/core.rs`, lines 500-509  
**Category:** Unsafe Deserialization / Type Confusion

The `JsonRpcMessage` enum uses `#[serde(untagged)]`, which means serde tries each variant in declaration order until one succeeds. The order is: `Request`, `Response`, `Notification`.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    Request(JsonRpcRequest),
    Response(JsonRpcResponse),
    Notification(JsonRpcNotification),
}
```

A JSON-RPC Response that happens to also have a `method` field will be deserialized as a `Request` instead, since `Request` is tried first and `JsonRpcRequest` has optional `params`. This can cause the interceptor pipeline to process a response as if it were a request, potentially triggering unintended routing or tool invocations.

**Impact:** A malicious upstream server could craft responses that are misclassified as requests, bypassing response-only interceptors or triggering request-path logic.

**Recommendation:** Implement a custom `Deserialize` that inspects discriminant fields (`id` + `method` = Request, `id` + `result`/`error` = Response, `method` without `id` = Notification) rather than relying on untagged trial-and-error.

---

## Finding 9: Interceptor Pipeline Silently Passes Through on Parse Failure (MEDIUM)

**File:** `acdp-transport/src/stdio_handler.rs`, lines 776-780  
**Category:** Security Bypass

When an incoming or outgoing message fails to parse as JSON-RPC, it is passed through unchanged without any interception:

```rust
Err(_) => {
    // Not valid JSON-RPC, pass through unchanged
    Ok((content.to_string(), false))
}
```

This means all interceptors (rate limiting, validation, LLM routing, blocking rules) are completely bypassed for any message that isn't valid JSON-RPC. An attacker who can inject non-JSON-RPC content into the stdio stream bypasses the entire security pipeline.

**Impact:** Complete bypass of all interceptors for non-JSON-RPC messages.

**Recommendation:** In security-sensitive deployments, non-JSON-RPC messages should be blocked rather than passed through. At minimum, log a warning at a higher severity level.

---

## Finding 10: `unsafe impl Send/Sync` on FFI Wrapper Without Verification (MEDIUM)

**File:** `acdp-llm/src/litert_wrapper.rs`, lines 263-265 and 407-408  
**Category:** Memory Safety

The `LiteRTEngine` and `LiteRTConversation` types have manual `unsafe impl Send` and `unsafe impl Sync`, with only a comment asserting thread safety:

```rust
// Safety: Engine can be sent between threads
unsafe impl Send for LiteRTEngine {}
unsafe impl Sync for LiteRTEngine {}
```

```rust
// Safety: Conversations can be moved between threads and shared
// The underlying C++ conversation is thread-safe
unsafe impl Send for LiteRTConversation {}
unsafe impl Sync for LiteRTConversation {}
```

The safety comments do not reference specific documentation or source code from the C++ LiteRT library that guarantees thread safety. If the underlying C++ objects are not actually thread-safe, concurrent access from Tokio tasks will cause data races and undefined behavior.

**Impact:** Potential data races, memory corruption, or crashes if the C++ library is not thread-safe.

**Recommendation:** Verify thread safety guarantees from the LiteRT documentation. If not confirmed, wrap the pointer in a `Mutex` instead of marking as `Sync`. Consider adding runtime tests for concurrent access.

---

## Finding 11: V8 Worker `fetch` Has No SSRF Protections (HIGH)

**File:** `acdp-sandbox/src/runtime/v8/worker.rs`, lines 12-30  
**Category:** SSRF (Server-Side Request Forgery)

*Note: While SSRF in worker.rs is listed as known, the specific mechanism documented here — the `op_fetch` and `op_fetch_request` ops — have additional concerns beyond basic SSRF.*

The V8 worker's fetch implementation performs unrestricted HTTP requests to any URL, including:
- Internal network addresses (`http://169.254.169.254/` for cloud metadata)
- Unix sockets (if reqwest supports it)
- Localhost services
- Arbitrary external endpoints

There are no URL allowlists, no IP blocklists, no protocol restrictions, and no request size limits on the response body.

```rust
async fn op_fetch(#[string] url: String) -> Result<String, JsErrorBox> {
    let response = reqwest::get(&url).await
        .map_err(|e| JsErrorBox::type_error(format!("Fetch failed: {}", e)))?;
    let body = response.text().await
        .map_err(|e| JsErrorBox::type_error(format!("Failed to read response: {}", e)))?;
```

Additionally, `op_fetch_request` allows arbitrary HTTP methods, headers, and request bodies to be sent, enabling more sophisticated attacks.

**Recommendation:** Implement URL validation that blocks private/internal IP ranges, cloud metadata endpoints, and localhost. Add response body size limits. Consider DNS rebinding protections.

---

## Finding 12: V8 Runtime Memory Leak via `Box::leak` (LOW)

**File:** `acdp-sandbox/src/runtime/v8/mod.rs`, lines 44-46 and 53-54  
**Category:** Resource Exhaustion

The V8 runtime uses `Box::leak()` to create `'static` lifetime references for snapshots:

```rust
pub fn with_snapshot(snapshot: Vec<u8>) -> Self {
    let snapshot_static: &'static [u8] = Box::leak(snapshot.into_boxed_slice());
    // ...
}
```

Every call to `with_snapshot()` or `with_snapshot_and_limits()` permanently leaks the snapshot data. If V8 runtimes are created and destroyed dynamically (e.g., per-request), this is a permanent memory leak.

**Recommendation:** Use `Arc<[u8]>` or store the `Box<[u8]>` alongside the runtime and use lifetime parameters instead of `'static`.

---

## Finding 13: IPC Deserialization Has No Message Size Limit (MEDIUM)

**File:** `acdp-common/src/ipc.rs`, lines 66-84  
**Category:** Denial of Service

The IPC `receive_message` function reads an entire line from the Unix socket with no size limit via `read_line()`:

```rust
pub async fn receive_message(&mut self) -> Result<Option<IpcEnvelope>> {
    let mut line = String::new();
    let bytes_read = self.reader.read_line(&mut line).await?;
    // ...
    match serde_json::from_str::<IpcEnvelope>(&line.trim()) {
```

`read_line()` will read until `\n` is found, allocating unbounded memory. A malicious or compromised IPC peer can send a single line of arbitrary length to exhaust memory.

**Recommendation:** Use `BufReader::take()` or `read_line` with a wrapper that limits the maximum line length (e.g., 10MB).

---

## Finding 14: Stdio Handler Reads Lines Without Size Limits (MEDIUM)

**File:** `acdp-transport/src/stdio_handler.rs`, lines 401-404  
**Category:** Denial of Service

The stdio handler reads from user stdin and child stdout using `read_line()` with no maximum line length:

```rust
let mut input = String::new();
let bytes_read = user_stdin.read_line(&mut input).await?;
```

Similarly in the core stdio transport (`acdp-core/src/transport/stdio.rs`, line 181):

```rust
let mut line = String::new();
match stdout_reader.read_line(&mut line).await {
```

A malicious MCP server (or compromised upstream) could send an extremely long line without `\n`, causing unbounded memory allocation.

**Recommendation:** Use a bounded read mechanism. Replace `read_line` with a loop that reads chunks and enforces a maximum message size (e.g., 10MB).

---

## Summary Table

| # | Severity | Category | Location | Finding |
|---|----------|----------|----------|---------|
| 1 | HIGH | Info Disclosure | wasm.rs:108 | WASI inherits host process args |
| 2 | HIGH | DoS | wasm.rs:46-56 | WASM memory limits never enforced |
| 3 | HIGH | Credential Leak | process.rs:50-56 | Process inherits full host environment |
| 4 | MEDIUM | Sandbox Escape | process.rs:50-56 | No working directory isolation |
| 5 | HIGH | DoS | http_sse_server.rs, http_stream_server.rs | No HTTP body size limits |
| 6 | MEDIUM | DoS | http_sse_server.rs:209-221 | Unbounded SSE session map |
| 7 | LOW | Injection | http_sse_server.rs:226-228 | Session ID not JSON-escaped in SSE |
| 8 | MEDIUM | Type Confusion | core.rs:500-509 | `#[serde(untagged)]` ordering on JsonRpcMessage |
| 9 | MEDIUM | Security Bypass | stdio_handler.rs:776-780 | Non-JSON-RPC messages bypass all interceptors |
| 10 | MEDIUM | Memory Safety | litert_wrapper.rs:263-265 | Unverified `unsafe impl Send/Sync` on FFI types |
| 11 | HIGH | SSRF | worker.rs:12-30 | Unrestricted fetch with no URL/IP filtering |
| 12 | LOW | Resource Leak | v8/mod.rs:44-46 | `Box::leak` for snapshots causes permanent memory leak |
| 13 | MEDIUM | DoS | ipc.rs:66-84 | Unbounded IPC message line reads |
| 14 | MEDIUM | DoS | stdio_handler.rs:401-404 | Unbounded stdio line reads |

**Total: 4 HIGH, 7 MEDIUM, 2 LOW**
