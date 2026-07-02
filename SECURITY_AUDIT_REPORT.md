# ACDP Security Audit Report

**Date:** 2026-07-02  
**Scope:** Authentication/authorization bypass and cryptographic weaknesses  
**Repository:** ACDP — MCP Proxy and Credential Gateway  

---

## Finding 1: No CORS Configuration on Any HTTP Server

**Severity:** HIGH  
**Category:** CORS Misconfiguration  

All four HTTP server implementations have **zero CORS configuration** — no CORS middleware, no `Access-Control-Allow-Origin` headers, no preflight handling. While the absence of CORS headers means browsers will *block* cross-origin JS from reading responses (the default same-origin policy), it also means:

- **No legitimate cross-origin clients can use the APIs** from a browser, making the gateway unusable for web-based MCP clients.
- **Simple requests** (e.g., POST with `Content-Type: application/json` may be treated as simple by some browsers) are still *sent* — the response is just blocked from JS. State-changing operations still execute.
- When CORS is eventually added, if done carelessly (wildcard `*`), all current endpoints will be wide-open.

### Affected files:

| File | Lines | Description |
|------|-------|-------------|
| `acdp-gateway/acdp-server/src/main.rs` | 123-135 | Actix-web app — no `actix-cors` middleware |
| `acdp-tui/src/http_server.rs` | 50-59 | Axum router — no `CorsLayer` |
| `acdp-transport/src/http_sse_server.rs` | 61-67 | Axum router — no `CorsLayer` |
| `acdp-transport/src/http_stream_server.rs` | 45-49 | Axum router — no `CorsLayer` |

**Note:** `tower-http` with the `cors` feature is listed in `acdp-tui/Cargo.toml` (line 35) but is **never imported or used** in any Rust source file.

**Recommendation:** Add explicit CORS middleware with an allowlist of trusted origins. Never use `*` for state-changing endpoints.

---

## Finding 2: No CSRF Protection on State-Changing Endpoints

**Severity:** HIGH  
**Category:** CSRF  

All POST endpoints (`/credentials/issue`, `/credentials/verify`, `/credentials/delegate`, `/mcp`, `/sse`) accept requests without any CSRF protection:

- No CSRF tokens
- No `SameSite` cookie attributes (no cookies used at all — bearer tokens only)
- No `Origin` or `Referer` header validation on the server side
- No custom header requirements that would prevent simple cross-origin requests

The `validate_origin()` method in `acdp-core/src/transport/http_sse.rs` (line 244) is a **no-op** — it logs a debug message but never actually validates anything:

```rust
fn validate_origin(&self, _request_builder: &reqwest::RequestBuilder) -> McpResult<()> {
    if !self.security_config.validate_origin {
        return Ok(());
    }
    // Only logs, never rejects
    tracing::debug!("Origin validation enabled for localhost connection");
    Ok(())
}
```

**Recommendation:** Implement `Origin` header validation on server-side endpoints. Require a custom header (e.g., `X-Requested-With`) for state-changing requests, which forces CORS preflight.

---

## Finding 3: Weak RNG Usage in Transport Name Generation

**Severity:** LOW  
**Category:** Weak Random Number Generation  

`acdp-transport/src/main.rs` lines 7-8, 49-54:

```rust
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};

let random_suffix: String = thread_rng()
    .sample_iter(&Alphanumeric)
    .take(6)
    .map(char::from)
    .collect();
```

`thread_rng()` delegates to `OsRng` on modern `rand` versions, so this is actually cryptographically secure. However, generating only 6 alphanumeric characters (≈36 bits of entropy) for a proxy name could lead to collisions if many proxies are spawned.

All **cryptographic** random generation in `acdp-auth` correctly uses `rand_core::OsRng` (CSPRNG), including `ServerPrivateKey::random()`, `ClientSecrets::random()`, and all scalar generation within ARC operations.

**Status:** ACCEPTABLE — no cryptographic weakness, only cosmetic proxy name collisions possible.

---

## Finding 4: ARC Server Private Key Generated at Startup, Never Persisted

**Severity:** HIGH  
**Category:** Key Management  

`acdp-gateway/acdp-server/src/services/credential.rs` lines 40-45:

```rust
let arc_server_key = Arc::new(ServerPrivateKey::random());
let arc_server_pubkey = Arc::new(ServerPublicKey::from_private_key(
    &arc_server_key,
    &arc_generators,
));
```

The ARC server private key is generated **randomly on every application startup** and is never persisted to disk or database. This means:

1. **All ARC credentials become unverifiable after a server restart** — the verification key changes.
2. An attacker can force credential invalidation by causing a server restart (DoS vector).
3. There is no way to perform key rotation or backup.

Similarly, in `acdp-auth/src/gateway.rs` line 122, the gateway keypair is generated fresh on each start:

```rust
let keypair = KeyPair::generate();
```

The `acdp-gateway/acdp-server` binary properly loads keys from environment variables (`ACDP_GATEWAY_SIGNING_KEY`, `ACDP_GATEWAY_PUBLIC_KEY`) for Ed25519, but the ARC keys have no such mechanism.

**Recommendation:** Persist ARC server keys to secure storage. Load them on startup like the Ed25519 signing keys.

---

## Finding 5: Timing Side-Channel in Credential/Principal Comparisons

**Severity:** MEDIUM  
**Category:** Timing Side-Channel  

Despite the `subtle` crate being listed as a dependency in `acdp-auth/Cargo.toml` (line 37), it is **never used** in any source file. All cryptographic comparisons use standard `==` or `!=` operators, which are not constant-time:

### 5a. Principal verification — `acdp-auth/src/principal.rs` lines 72-91:

```rust
if self.human_id != sub { ... }
if self.idp_issuer != iss { ... }
if self.idp_client_id != client_id { ... }
```

Early-return on mismatch leaks which field failed via timing.

### 5b. Token type check — `acdp-auth/src/mcp.rs` line 108:

```rust
if self.token_type != "oauth-id-jag+jwt" { ... }
```

### 5c. ARC presentation tag comparison — `acdp-auth/src/arc.rs` line 631:

```rust
if self.t != expected_t || self.m1_tag != expected_m1_tag {
    return Ok(false);
}
```

This compares elliptic curve points using the standard `PartialEq` trait, which is not guaranteed to be constant-time. While P-256 point comparisons are typically done on field elements (which *may* be constant-time in the `p256` crate internally), this is not documented or guaranteed.

**Recommendation:** Use `subtle::ConstantTimeEq` for all security-critical comparisons, especially the ARC tag verification.

---

## Finding 6: Credential Verify Endpoint Has No Authentication

**Severity:** HIGH  
**Category:** Missing Authentication  

`acdp-gateway/acdp-server/src/routes/credential_verify.rs` — The `verify_credential` endpoint requires **no authentication whatsoever**:

```rust
#[post("/credentials/verify")]
pub async fn verify_credential(
    state: web::Data<AppState>,
    body: web::Json<CredentialVerificationRequest>,
) -> Result<impl Responder> { ... }
```

Unlike the `issue_credential` endpoint (which checks an `Authorization: Bearer` header), the verify endpoint is completely open. Any unauthenticated client can:

1. **Enumerate valid credential IDs** by submitting verification requests and observing different error messages ("not found" vs "expired" vs "revoked").
2. **Exhaust rate limits** by sending repeated verify requests to increment the `presentations_used` counter without needing to hold the actual credential.
3. **Denial-of-service** legitimate credential holders by burning their presentation quota.

**Also applies to:** `acdp-gateway/acdp-server/src/routes/delegation.rs` — the delegation endpoint also has no auth (though it's unimplemented).

**Recommendation:** Require MCP server authentication (e.g., server-to-server API key or mTLS) on the verify endpoint.

---

## Finding 7: No TLS Minimum Version Enforcement

**Severity:** MEDIUM  
**Category:** TLS Configuration  

The ACME/TLS setup in `acdp-tui/src/http_server.rs` lines 612-732 uses `rustls-acme` with default TLS configuration:

```rust
let mut state = acme_config.state();
let acceptor = state.axum_acceptor(state.default_rustls_config());
```

`default_rustls_config()` relies on rustls defaults. While rustls is generally safe (no SSL 3.0 or TLS 1.0/1.1), there is:

- No explicit minimum TLS version set (e.g., `TLS 1.3` only).
- No cipher suite restrictions.
- No certificate pinning for upstream connections.

The `reqwest` client in `acdp-gateway/acdp-server/src/services/rauthy_client.rs` line 20 uses default configuration:

```rust
client: reqwest::Client::new(),
```

This doesn't set `min_tls_version`, doesn't pin certificates, and would accept any valid TLS certificate for the Rauthy server.

**Recommendation:** Set minimum TLS version to 1.3 for the ACME server. For the Rauthy client, consider certificate pinning if the Rauthy instance is self-hosted.

---

## Finding 8: Hardcoded Fallback Secrets and Placeholder Values

**Severity:** MEDIUM  
**Category:** Hardcoded Secrets  

### 8a. Hardcoded ACME contact email — `acdp-tui/src/http_server.rs` line 636:

```rust
vec!["mailto:admin@example.com".to_string()]
```

If no email is configured for TLS/ACME, the system falls back to a fake email address. Let's Encrypt rate-limits by email and domain, and `admin@example.com` may be blacklisted or used by many unrelated systems.

### 8b. Hardcoded default session ID — `acdp-core/src/transport/http_sse.rs` line 1459:

```rust
self.session_id = Some("default".to_string());
```

When no session is discovered from a legacy server, the transport falls back to the static string `"default"`. This session ID is predictable and could be used by an attacker to hijack sessions if the server trusts session IDs without validation.

### 8c. All-zeros example signing key — `acdp-gateway/.env.example` line 17:

```
ACDP_GATEWAY_SIGNING_KEY=0000000000000000000000000000000000000000000000000000000000000000
```

While this is in an `.env.example` file, if copied as-is to `.env`, the gateway would use the all-zeros key for signing, which is a well-known weak key.

**Recommendation:** Remove the default ACME email fallback (fail loudly instead). Reject weak/all-zeros signing keys at startup. Use cryptographically random session IDs instead of `"default"`.

---

## Finding 9: Rauthy Client Validation Bypass — Always Returns `true`

**Severity:** CRITICAL  
**Category:** Authentication Bypass  

`acdp-gateway/acdp-server/src/services/rauthy_client.rs` lines 60-69:

```rust
pub async fn validate_client(
    &self,
    client_id: &str,
    client_secret: &str,
) -> Result<bool> {
    // TODO: Implement client validation via Rauthy API
    Ok(true) // Placeholder
}
```

The `validate_client` method is a **stub that always returns `Ok(true)`**, meaning any client ID and secret combination is accepted as valid. While this method may not be called in all code paths today, it represents a **latent auth bypass** — any future code that relies on it for access control will be ineffective.

Additionally, `verify_id_token()` (lines 25-35) always returns an error, meaning ID token verification is completely non-functional:

```rust
pub async fn verify_id_token(&self, id_token: &str) -> Result<IDTokenClaims> {
    Err(ACDPGatewayError::RauthyError(
        "ID token verification not yet implemented".to_string(),
    ))
}
```

**Recommendation:** Implement actual Rauthy token introspection. Remove the `Ok(true)` stub immediately or mark it as `unimplemented!()` to prevent accidental use.

---

## Finding 10: No Security Headers on Any HTTP Response

**Severity:** MEDIUM  
**Category:** Missing Security Headers  

None of the HTTP servers set any security headers. The following headers are **completely absent** from all responses across all server implementations:

| Header | Purpose |
|--------|---------|
| `X-Content-Type-Options: nosniff` | Prevent MIME type sniffing |
| `X-Frame-Options: DENY` | Prevent clickjacking |
| `Strict-Transport-Security` | Enforce HTTPS (HSTS) |
| `Content-Security-Policy` | Prevent XSS |
| `X-XSS-Protection` | Legacy XSS protection |
| `Referrer-Policy` | Control referrer leakage |
| `Permissions-Policy` | Restrict browser features |
| `Cache-Control: no-store` | Prevent caching of credentials |

**Affected files:**
- `acdp-gateway/acdp-server/src/main.rs` — only has `Logger` and `Compress` middleware
- `acdp-tui/src/http_server.rs` — no middleware at all
- `acdp-transport/src/http_sse_server.rs` — no middleware at all
- `acdp-transport/src/http_stream_server.rs` — no middleware at all

**Recommendation:** Add a security headers middleware to all HTTP servers. For Actix, use `actix-web-security-headers`. For Axum, use `tower-http`'s `SetResponseHeader` layer. At minimum, add `X-Content-Type-Options`, `X-Frame-Options`, and `Cache-Control: no-store` on credential responses.

---

## Finding 11: Signature Data Mismatch Between Issuance and Verification

**Severity:** HIGH  
**Category:** Cryptographic Weakness  

The `CredentialService` in `acdp-gateway/acdp-server/src/services/credential.rs` signs identity-bound credentials with a different data format than what `IdentityBoundCredential::verify_signature()` in `acdp-auth/src/credentials.rs` expects.

### Issuance signing data (credential.rs lines 152-156):

```rust
let mut signing_data = Vec::new();
signing_data.extend_from_slice(b"ACDP-v0.3");
signing_data.extend_from_slice(credential_id.as_bytes());
signing_data.extend_from_slice(&issued_at.timestamp().to_le_bytes());
signing_data.extend_from_slice(&expires_at.timestamp().to_le_bytes());
```

### Verification signing data (credentials.rs lines 183-204):

```rust
fn signing_data(&self) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    data.extend_from_slice(self.version.as_bytes());  // "0.3" not "ACDP-v0.3"
    data.extend_from_slice(self.credential_id.as_bytes());
    data.extend_from_slice(&self.issued_at.timestamp().to_le_bytes());
    data.extend_from_slice(&self.expires_at.timestamp().to_le_bytes());
    let canonical = serde_json::to_vec(&(
        &self.principal, &self.agent, &self.mcp_capabilities, &self.delegation,
    ))?;
    data.extend_from_slice(&canonical);
    Ok(data)
}
```

**Differences:**
1. Issuance uses `b"ACDP-v0.3"` (9 bytes); verification uses `self.version.as_bytes()` which is `"0.3"` (3 bytes).
2. Verification includes `principal`, `agent`, `mcp_capabilities`, and `delegation` in the signing data; issuance **omits all of these**.

This means **signature verification will always fail** for credentials issued by the gateway service, effectively making the verification endpoint unable to cryptographically validate any credential.

For hybrid credentials, the issuance signing data is even shorter (only `b"ACDP-v0.3"` + credential_id, without timestamps), further diverging from the verification format.

In the `acdp-auth` gateway, the `sign_credential` method signs **only the credential UUID** (16 bytes):

```rust
fn sign_credential(&self, credential_id: &Uuid) -> Result<Signature> {
    let data = credential_id.as_bytes();
    Ok(self.keypair.sk.sign(data, None))
}
```

This means the signature covers none of the credential's security-critical fields (principal, agent, capabilities, expiration), allowing any of those to be tampered with.

**Recommendation:** Unify the signing data format between issuance and verification. Both must use the identical canonicalization that includes all security-critical fields.

---

## Finding 12: ID-JAG JWT Validation Disables Standard Checks

**Severity:** MEDIUM  
**Category:** JWT Validation Weakness  

`acdp-auth/src/mcp.rs` lines 144-146:

```rust
let mut validation = Validation::default();
validation.validate_exp = false;
validation.validate_aud = false;
```

The `IDJAGToken::decode()` method disables both expiration and audience validation during JWT decoding. While the code comments claim these are "validated manually" via the `verify()` method, this creates a window where expired or misaddressed tokens are accepted if `decode()` is called without a subsequent `verify()` call.

In the `id_jag.rs` service (`acdp-gateway/acdp-server`), the `validate_id_jag()` function correctly enables `validate_exp = true` and sets the audience. However, the `IDJAGToken::decode()` method in the auth library does not, which means any code using the library directly (including the gateway binary at `acdp-auth/src/bin/gateway.rs`) may accept expired/misaddressed tokens.

Additionally, `Validation::default()` uses HS256 as the default algorithm, meaning the `jsonwebtoken` crate will reject tokens signed with RSA/EC unless explicitly configured. This is correct for the current `DecodingKey::from_secret()` usage, but will break when real IdP keys (typically RS256/ES256) are used.

**Recommendation:** Never disable standard JWT validation checks. If manual validation is needed, perform it *before* accepting the token. Configure the algorithm list to match the expected IdP signing algorithm.

---

## Summary of Findings

| # | Finding | Severity | Category |
|---|---------|----------|----------|
| 1 | No CORS configuration on any HTTP server | HIGH | CORS |
| 2 | No CSRF protection on state-changing endpoints | HIGH | CSRF |
| 3 | Weak RNG in proxy name generation | LOW | RNG |
| 4 | ARC server private key not persisted | HIGH | Key Management |
| 5 | Timing side-channel in crypto comparisons | MEDIUM | Side-Channel |
| 6 | Verify/delegate endpoints have no authentication | HIGH | Auth Bypass |
| 7 | No TLS minimum version enforcement | MEDIUM | TLS |
| 8 | Hardcoded fallback secrets and placeholders | MEDIUM | Secrets |
| 9 | Rauthy `validate_client` always returns true | CRITICAL | Auth Bypass |
| 10 | No security headers on any HTTP response | MEDIUM | Headers |
| 11 | Signature data mismatch between issuance/verification | HIGH | Crypto |
| 12 | ID-JAG JWT decode disables exp/aud validation | MEDIUM | JWT |

---

*Report generated by automated security audit on 2026-07-02.*
