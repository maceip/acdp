# Security Audit: CORS, Deserialization, Header Injection & Related Vulnerabilities

**Date:** 2026-07-24
**Scope:** All HTTP servers, transport clients, gateway, sandbox, and deserialization paths
**Severity scale:** Critical / High / Medium / Low / Informational

---

## Executive Summary

This audit covers CORS configuration, unsafe deserialization, header injection, request smuggling, and SSRF vectors across the ACDP Rust codebase. The most significant findings are:

1. **Complete absence of CORS middleware** on all four axum-based HTTP servers (Critical)
2. **Full SSRF via V8 worker `fetch()` with no URL restrictions** (Critical)
3. **No request body size limits** on any HTTP server (High)
4. **Header injection via `Mcp-Session-Id`** set from unvalidated user input (Medium)
5. **Unbounded deserialization** of JSON-RPC messages from untrusted stdio/network input (Medium)

---

## 1. CORS Configuration

### Finding 1.1 — No CORS headers on any HTTP server (CRITICAL)

**Affected files:**
- `acdp-transport/src/http_sse_server.rs` (lines 61–67)
- `acdp-transport/src/http_stream_server.rs` (lines 45–49)
- `acdp-tui/src/http_server.rs` (lines 50–60)
- `acdp-transport/src/bin/litert_mcp_server.rs` (lines 148–151)
- `acdp-gateway/acdp-server/src/main.rs` (lines 123–138)

**Detail:** None of the five HTTP server routers apply CORS middleware. The `tower-http` crate with the `cors` feature is declared as a dependency in `acdp-tui/Cargo.toml` but is **never imported or used** anywhere in the codebase. The `actix-web` gateway server also has no CORS middleware.

Without CORS headers, browsers will block cross-origin JavaScript from accessing these endpoints. However, the *lack* of explicit CORS configuration is a double-edged issue:

- **If these servers are intended to be accessed cross-origin** (e.g., from a web-based MCP client), they will fail to function.
- **If CORS is later added permissively** (e.g., `allow_any_origin()` + `allow_credentials(true)`), it creates a credential-forwarding vulnerability.
- **Without CORS, CSRF-style attacks via `<form>` POST are still possible** because simple content types bypass the preflight check — and `application/json` POSTs from forms won't trigger preflight either when the server lacks CORS enforcement.

**Recommendation:**
Add explicit `CorsLayer` (tower-http) or `actix_cors::Cors` to every HTTP server. Configure `allowed_origins` to a strict allowlist (not wildcard). Never combine `allow_any_origin()` with `allow_credentials(true)`.

### Finding 1.2 — SecurityConfig.allowed_origins is declared but unused (LOW)

**File:** `acdp-core/src/transport/http_sse.rs` (lines 123–125)

```rust
/// Allowed origins for CORS (used for SSE security validation)
#[allow(dead_code)]
allowed_origins: Vec<String>,
```

The field is marked `#[allow(dead_code)]` and is never checked against incoming requests. The `validate_origin()` method (line 244) is a no-op — it logs a debug message but performs no validation.

---

## 2. Unsafe Deserialization

### Finding 2.1 — No body size limit on any axum HTTP server (HIGH)

**Affected files:**
- `acdp-transport/src/http_sse_server.rs:92` — `Json(body): Json<Value>`
- `acdp-transport/src/http_stream_server.rs:74` — `Json(body): Json<Value>`
- `acdp-tui/src/http_server.rs:66` — `Json(body): Json<Value>`
- `acdp-transport/src/bin/litert_mcp_server.rs:173` — `Json(body): Json<Value>`

**Detail:** Axum's default body size limit is 2 MB, but the handlers accept `Json<Value>` which deserializes an **arbitrary** JSON tree into memory. A deeply nested JSON document (e.g., `{"a":{"a":{"a":...}}}` thousands of levels deep) can cause a **stack overflow** during `serde_json` deserialization, crashing the server. A very wide JSON document within the 2 MB limit can also exhaust memory through `Value` allocations.

No `DefaultBodyLimit` layer is configured to restrict this further. The `actix-web` gateway server also relies on the default 256 KB limit from actix, with no explicit `PayloadConfig` or `JsonConfig`.

**Recommendation:**
- Add `DefaultBodyLimit::max(bytes)` to all axum routers.
- Add `PayloadConfig` and `JsonConfig` with explicit limits to the actix-web gateway.
- Consider using `serde_json::from_str` with a `serde_json::Deserializer` that has `disable_recursion_limit` set to false (the default) and potentially use a crate like `serde_json_lenient` with configurable depth limits if stack overflow is a concern.

### Finding 2.2 — Unbounded `serde_json::from_str` on untrusted stdio input (MEDIUM)

**Affected files:**
- `acdp-tui/src/client_proxy.rs:229` — `serde_json::from_str(trimmed)`
- `acdp-tui/src/client_proxy.rs:285` — `serde_json::from_str(trimmed)`
- `acdp-transport/src/stdio_handler.rs:749` — `serde_json::from_str::<JsonRpcMessage>(content.trim())`
- `acdp-transport/src/backend_connection.rs:110` — `serde_json::from_str(trimmed)`
- `acdp-core/src/transport/stdio.rs:190` — `serde_json::from_str::<JsonRpcMessage>(trimmed)`

**Detail:** These calls read lines from `BufReader` over stdio/TCP without any size limit on the line itself. A malicious backend or client could send a single line containing an arbitrarily large JSON document, consuming unbounded memory. There is no `read_line` byte limit.

**Recommendation:** Use `take()` on the reader to impose a maximum line length, or use a length-prefixed framing protocol.

### Finding 2.3 — `bincode` dependency in `acdp-auth` (INFORMATIONAL)

**File:** `acdp-auth/Cargo.toml:24`

The `bincode` v2 crate is listed as a dependency but is **not used** anywhere in the `acdp-auth/src/` source files (grep returns no matches). This is a dead dependency. If `bincode` were used to deserialize untrusted input, it would be a high-risk issue because bincode can trigger large allocations from length-prefix fields. Since it's unused, this is informational only.

### Finding 2.4 — Credential JSON deserialization from user-controlled string (MEDIUM)

**File:** `acdp-gateway/acdp-server/src/routes/credential_verify.rs:17`

```rust
let credential: ACDPCredential = serde_json::from_str(&body.credential)
```

The `body.credential` field is a string submitted by the client in the JSON POST body. It is then deserialized via `serde_json::from_str` into an `ACDPCredential`. This is double-deserialization (the outer `web::Json` extracts the JSON body, and then an inner string field is deserialized again). The inner string has no size validation before deserialization.

**Recommendation:** Validate the length of `body.credential` before calling `from_str`, and consider accepting the credential as a nested JSON value rather than a string-encoded JSON.

---

## 3. Header Injection

### Finding 3.1 — `Mcp-Session-Id` response header set from unvalidated user input (MEDIUM)

**Affected files:**
- `acdp-transport/src/http_sse_server.rs:183` — `.header("Mcp-Session-Id", session_id)`
- `acdp-transport/src/http_stream_server.rs:130,214` — `.header("mcp-session-id", session_id)`
- `acdp-tui/src/http_server.rs:281,299` — `.header("Mcp-Session-Id", session_id)`
- `acdp-transport/src/bin/litert_mcp_server.rs:222` — `.header("Mcp-Session-Id", session.id.clone())`

**Detail:** The `session_id` is extracted from the incoming `Mcp-Session-Id` request header (or generated if absent). When present, the user-supplied value is placed directly into the response header. In `http_sse_server.rs:106-110`:

```rust
let session_id = headers
    .get("Mcp-Session-Id")
    .and_then(|h| h.to_str().ok())
    .map(|s| s.to_string())
    .unwrap_or_else(|| Uuid::new_v4().to_string());
```

The `h.to_str().ok()` check prevents non-ASCII bytes, and axum's `HeaderValue` rejects `\r\n` characters. This provides **partial mitigation**: classical CRLF header injection is not possible because `http::HeaderValue::from_str` rejects control characters. However:

- The session_id is reflected without sanitization, making it a **header value reflection** vector.
- If the session ID is logged or displayed in UI (as it is in `acdp-tui`), it could be used for log injection.
- The `http_stream_server.rs:89-93` handler also accepts `mcp-session-id` (lowercase) as a fallback, widening the surface.

**Mitigating factor:** Axum's `http` crate validates header values and rejects control characters, so classic `\r\n` injection is blocked at the HTTP level.

**Recommendation:** Validate `session_id` format (UUID pattern) before reflecting it in response headers. The `validate_session_id()` in `http_sse.rs:261-284` has this logic but it is only used on the client transport side, not on the server side.

### Finding 3.2 — User-controlled HTTP headers in V8 worker `fetch()` (HIGH)

**File:** `acdp-sandbox/src/runtime/v8/worker.rs:64-71`

```rust
if let Some(headers_map) = headers_obj.as_object() {
    for (key, value) in headers_map {
        if let Some(val_str) = value.as_str() {
            request = request.header(key, val_str);
        }
    }
}
```

JavaScript code running in the V8 sandbox can set arbitrary HTTP headers on outgoing requests through the `fetch()` API. There is no filtering of security-sensitive headers like `Host`, `Authorization`, `Cookie`, `X-Forwarded-For`, etc. While `reqwest` will reject header values with CRLF, the ability to set any header name enables:

- Injecting `Host` headers for cache-poisoning attacks
- Setting `Authorization` headers to impersonate users on internal services
- Adding `X-Forwarded-For` to bypass IP-based access controls

**Recommendation:** Maintain a blocklist of forbidden header names (at minimum: `Host`, `Authorization`, `Cookie`, `Proxy-Authorization`, `X-Forwarded-*`) and reject them in `op_fetch_request`.

---

## 4. Request Smuggling

### Finding 4.1 — Reliance on framework defaults (LOW)

**Detail:** All production HTTP servers use either `axum::serve()` or `actix_web::HttpServer`, both of which use `hyper` under the hood. Hyper handles `Transfer-Encoding` and `Content-Length` correctly per HTTP/1.1 spec and rejects ambiguous combinations. No custom Transfer-Encoding handling is present.

The example file `acdp-sandbox/examples/v8_worker.rs:223-315` implements a **manual HTTP parser** over raw `TcpStream` that parses `Content-Length` headers manually (`parse_content_length`). This is vulnerable to request smuggling if deployed in production (it doesn't handle chunked transfer encoding, multiple Content-Length headers, or CL/TE conflicts). However, this is an example file, not production code.

**Recommendation:** Ensure the `v8_worker.rs` example code is never used in production. Add a comment noting it is not suitable for production use.

---

## 5. SSRF (Server-Side Request Forgery)

### Finding 5.1 — V8 worker `fetch()` allows arbitrary URL requests with no restrictions (CRITICAL)

**File:** `acdp-sandbox/src/runtime/v8/worker.rs:12-30`

```rust
async fn op_fetch(#[string] url: String) -> Result<String, JsErrorBox> {
    let response = reqwest::get(&url).await ...
```

And `op_fetch_request` (lines 35-93):

```rust
let client = reqwest::Client::new();
let mut request = match method.to_uppercase().as_str() {
    "GET" => client.get(&url),
    "POST" => client.post(&url),
    ...
```

**Detail:** The V8 worker sandbox exposes a `fetch()` API that makes outbound HTTP requests to **any URL** provided by the JavaScript code. There are zero protections against:

1. **Internal network access**: `fetch("http://169.254.169.254/latest/meta-data/")` to access cloud instance metadata
2. **Localhost access**: `fetch("http://localhost:8080/admin")` to reach internal services
3. **DNS rebinding**: An attacker-controlled domain that resolves to `127.0.0.1`
4. **Redirect-based SSRF**: `reqwest::Client::new()` follows redirects by default (up to 10). A URL can redirect to internal addresses.
5. **Non-HTTP schemes**: While `reqwest` only supports HTTP(S), the URL parsing doesn't validate this.

The `NetworkPolicy` enum in `acdp-common/src/gencode.rs` defines `Disabled`/`Limited`/`Full` policies, and the code in `acdp-sandbox/src/service.rs:198` checks this policy — but the V8 worker's `op_fetch` and `op_fetch_request` ops **completely bypass this policy check**. They directly call `reqwest` without any policy enforcement.

**Recommendation:**
- Validate URLs before making requests: block private IP ranges (10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 169.254.0.0/16, 127.0.0.0/8, ::1), link-local, and non-HTTP schemes.
- Disable redirect following or validate redirect targets: `reqwest::Client::builder().redirect(reqwest::redirect::Policy::none())`.
- Enforce the `NetworkPolicy` from the execution context within the V8 ops.
- Consider using a DNS resolver that blocks private IPs to prevent DNS rebinding.

### Finding 5.2 — `reqwest::Client::new()` used without redirect restrictions (MEDIUM)

**Affected files:**
- `acdp-llm/src/model_management.rs:179,242,369` — `reqwest::Client::new()`
- `acdp-gateway/acdp-server/src/services/rauthy_client.rs:20` — `reqwest::Client::new()`
- `acdp-sandbox/src/runtime/v8/worker.rs:48` — `reqwest::Client::new()`

**Detail:** All `reqwest::Client::new()` instances use the default configuration which follows up to 10 redirects. If any URL is constructed from user input (directly the case for V8 worker, indirectly possible for model download URLs), an attacker can chain redirects to reach internal services.

The `RauthyClient` at `acdp-gateway/acdp-server/src/services/rauthy_client.rs` constructs URLs from the `RAUTHY_BASE_URL` environment variable (line 39: `format!("{}/oidc/userinfo", self.base_url)`), which is trusted configuration. However, if Rauthy itself returns a redirect to an internal address, the client will follow it.

**Recommendation:** Configure `reqwest::Client` with `redirect(Policy::none())` or `redirect(Policy::limited(0))` for security-sensitive clients. For model downloads, validate the URL scheme and host before following redirects.

### Finding 5.3 — `validate_client` always returns `Ok(true)` (HIGH)

**File:** `acdp-gateway/acdp-server/src/services/rauthy_client.rs:60-69`

```rust
pub async fn validate_client(&self, client_id: &str, client_secret: &str) -> Result<bool> {
    // TODO: Implement client validation via Rauthy API
    Ok(true) // Placeholder
}
```

This is an authentication bypass. Any `client_id` and `client_secret` will pass validation. While marked as a TODO placeholder, this is extremely dangerous if the method is called in any authorization flow.

**Recommendation:** Implement proper client validation or return `Ok(false)` / `Err(...)` as a safe default until implemented.

---

## 6. Additional Findings

### Finding 6.1 — Session HashMap grows without bound (MEDIUM)

**Affected files:**
- `acdp-transport/src/http_sse_server.rs:43` — `sessions: Arc<Mutex<HashMap<String, HttpSession>>>`
- `acdp-tui/src/http_server.rs:46` — `sessions: Arc<Mutex<HashMap<String, HttpSession>>>`
- `acdp-transport/src/bin/litert_mcp_server.rs:62` — `sessions: Arc<Mutex<HashMap<String, Session>>>`

**Detail:** Sessions are added to the HashMap but never removed (no timeout or eviction). An attacker can send requests with unique `Mcp-Session-Id` values to create unbounded session entries, eventually exhausting memory. Each session includes a `broadcast::channel(100)` which allocates buffer capacity.

**Recommendation:** Implement session timeouts and eviction. Limit the maximum number of concurrent sessions.

### Finding 6.2 — `ProcessBackendConnection::spawn` uses unsanitized shell commands (HIGH)

**File:** `acdp-transport/src/backend_connection.rs:60-66`

```rust
let mut child = Command::new("sh")
    .arg("-c")
    .arg(command)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()?;
```

The `command` parameter is passed directly to `sh -c` without any sanitization. If `command` is derived from user input anywhere in the call chain, this is a command injection vulnerability. The `command` appears to come from configuration (`backend_command` in `acdp-tui/src/http_server.rs:155`), which is operator-controlled, but should still be documented as a trust boundary.

### Finding 6.3 — `DecodingKey::from_secret(b"test-secret")` hardcoded (CRITICAL — if deployed)

**File:** `acdp-gateway/acdp-server/src/routes/credential_issue.rs:35`

```rust
let idp_public_key = DecodingKey::from_secret(b"test-secret");
```

This hardcoded test secret is used to validate ID-JAG tokens. Any attacker who knows this secret (which is in source code) can forge valid tokens. The comment says "TODO: Get IdP public key dynamically", but if this code is deployed as-is, it's a critical authentication bypass.

---

## Summary Table

| # | Finding | Severity | File(s) |
|---|---------|----------|---------|
| 1.1 | No CORS middleware on any HTTP server | Critical | All server files |
| 1.2 | `allowed_origins` field unused | Low | `http_sse.rs:123` |
| 2.1 | No body size limits | High | All axum handlers |
| 2.2 | Unbounded `from_str` on stdio | Medium | `client_proxy.rs`, `stdio_handler.rs`, `backend_connection.rs` |
| 2.3 | Unused `bincode` dependency | Info | `acdp-auth/Cargo.toml` |
| 2.4 | Double deserialization of credential | Medium | `credential_verify.rs:17` |
| 3.1 | Session ID reflected in headers | Medium | All server files |
| 3.2 | User-controlled headers in V8 fetch | High | `worker.rs:64-71` |
| 4.1 | Manual HTTP parser in example | Low | `v8_worker.rs:223` |
| 5.1 | Unrestricted SSRF via V8 fetch | Critical | `worker.rs:12-93` |
| 5.2 | Default redirect following | Medium | Multiple `reqwest::Client::new()` |
| 5.3 | `validate_client` always true | High | `rauthy_client.rs:60` |
| 6.1 | Unbounded session HashMap | Medium | All server session stores |
| 6.2 | Shell command injection surface | High | `backend_connection.rs:60` |
| 6.3 | Hardcoded `test-secret` | Critical | `credential_issue.rs:35` |
