# ACDP Security Audit Report

**Date:** 2026-07-22
**Scope:** Cryptographic weaknesses, secret management, configuration security
**Auditor:** Automated security analysis

---

## Finding 1: JWT Decode Disables Expiration and Audience Validation

**File:** `acdp-auth/src/mcp.rs` lines 143–152
**Severity:** HIGH

```rust
pub fn decode(token: &str, verification_key: &DecodingKey) -> Result<Self> {
    let mut validation = Validation::default();
    validation.validate_exp = false; // We'll validate manually
    validation.validate_aud = false; // Audience validated in verify()

    let token_data = decode::<IDJAGToken>(token, verification_key, &validation)
        .map_err(|e| ACDPError::InvalidIDJAG(format!("Failed to decode JWT: {}", e)))?;

    Ok(token_data.claims)
}
```

**Attacker-controlled input:** Yes — the `token` string is attacker-supplied.

**Existing controls:** Comments say "we'll validate manually" and there is a separate `verify()` method. However, `decode()` is a public function and callers can use the returned `IDJAGToken` without calling `verify()`. Nothing enforces the two-step pattern.

**Attack path:** A caller invokes `IDJAGToken::decode()` and uses the claims without calling `verify()`. An expired or audience-mismatched token is accepted. The `jsonwebtoken` crate's built-in protections are deliberately disabled. Since the `Validation::default()` algorithm list accepts HS256 only and the signing key type determines what's accepted, this creates an additional concern: without algorithm pinning via `set_required_spec_claims`, a misconfigured `DecodingKey` could allow algorithm confusion.

---

## Finding 2: JWT `encode()` Uses `Header::default()` (HS256) — No Algorithm Pinning

**File:** `acdp-auth/src/mcp.rs` line 138
**Severity:** MEDIUM

```rust
pub fn encode(&self, signing_key: &EncodingKey) -> Result<String> {
    encode(&Header::default(), self, signing_key)
        .map_err(|e| ACDPError::TokenExchangeFailed(format!("Failed to encode JWT: {}", e)))
}
```

**Attacker-controlled input:** No — this is token creation, not verification.

**Existing controls:** None. `Header::default()` uses `Algorithm::HS256`.

**Attack path:** The ID-JAG specification likely expects asymmetric signatures (RS256/ES256) for federation trust. Using HMAC (symmetric) means the signing secret and verification key are identical. Any party that can verify tokens can also forge them. This is a design mismatch: an enterprise IdP JWT should use asymmetric cryptography so the gateway can verify tokens without possessing the IdP's signing key.

---

## Finding 3: Gateway Credential Issuance Route Hardcodes `test-secret` as IdP Key (Production Code Path)

**File:** `acdp-gateway/acdp-server/src/routes/credential_issue.rs` lines 33–35
**Severity:** CRITICAL

```rust
// TODO: Get IdP public key dynamically based on token issuer
// For now, use a placeholder key
let idp_public_key = DecodingKey::from_secret(b"test-secret");
```

**Attacker-controlled input:** Yes — `id_jag_token` from the Authorization header.

**Existing controls:** None. The TODO comment acknowledges this is intentionally insecure.

**Attack path:** Any attacker who knows the string `test-secret` (visible in the source code) can forge valid ID-JAG tokens, impersonate any user principal, and receive arbitrary ACDP credentials. Since this is in the production `/credentials/issue` endpoint (not just a test), it is directly exploitable.

---

## Finding 4: Client Validation Always Returns `Ok(true)` (Authentication Bypass)

**File:** `acdp-gateway/acdp-server/src/services/rauthy_client.rs` lines 60–69
**Severity:** HIGH

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

**Attacker-controlled input:** Yes — `client_id` and `client_secret` come from the request.

**Existing controls:** None.

**Attack path:** Any client presenting arbitrary credentials passes validation. Combined with Finding 3, a completely unauthenticated attacker can issue ACDP credentials for any identity.

---

## Finding 5: Gateway Signing Data Excludes Principal, Agent, and Capabilities (Credential Tampering)

**File:** `acdp-gateway/acdp-server/src/services/credential.rs` lines 152–160
**Severity:** HIGH

```rust
let mut signing_data = Vec::new();
signing_data.extend_from_slice(b"ACDP-v0.3");
signing_data.extend_from_slice(credential_id.as_bytes());
signing_data.extend_from_slice(&issued_at.timestamp().to_le_bytes());
signing_data.extend_from_slice(&expires_at.timestamp().to_le_bytes());

let keypair = KeyPair::from_seed(
    ed25519_compact::Seed::from_slice(&self.signing_key).unwrap()
);
let signature = keypair.sk.sign(&signing_data, None);
```

**Attacker-controlled input:** The credential is returned to the client; the client controls the serialized credential going forward.

**Existing controls:** The `IdentityBoundCredential::signing_data()` method in `acdp-auth/src/credentials.rs` (line 183–204) does include principal/agent/capabilities via serde_json. However, the **gateway server** uses a different, minimal signing routine that excludes these fields.

**Attack path:** After receiving a legitimately issued credential, an attacker can modify the `principal`, `agent`, `mcp_capabilities`, or `delegation` fields. The signature only covers version, credential_id, and timestamps. Verification will still pass because those fields haven't changed.

The hybrid credential signing (lines 279–284) is even worse — it omits even `issued_at` and `expires_at`:

```rust
let mut signing_data = Vec::new();
signing_data.extend_from_slice(b"ACDP-v0.3");
signing_data.extend_from_slice(credential_id.as_bytes());
```

---

## Finding 6: Signing Data Mismatch Between Issuance and Verification

**File:** `acdp-gateway/acdp-server/src/services/credential.rs` (issuance) vs `acdp-auth/src/credentials.rs` lines 183–204 (verification)
**Severity:** HIGH

The gateway server signs a minimal `{version, credential_id, timestamps}` blob. The `IdentityBoundCredential::verify_signature()` method verifies against a completely different blob that includes `{version, credential_id, timestamps, principal, agent, capabilities, delegation}` via serde_json.

**Attack path:** Every legitimately issued credential will **fail** verification through the library's `verify_signature()` method because the signed data doesn't match. This means either: (a) the verification path is never actually used in production, or (b) some other verification path is used. Either way, the system cannot securely verify credential integrity.

---

## Finding 7: Downloaded Models Are Never Verified for Integrity

**File:** `acdp-llm/src/model_management.rs` lines 338–455
**Severity:** HIGH

```rust
pub async fn download_model(&self, model_name: &str) -> LlmResult<DownloadProgress> {
    // ... download to temp_path ...
    // Rename temp file to final name
    tokio::fs::rename(&temp_path, &model_path).await ...
}
```

**Attacker-controlled input:** The model file content comes from `https://huggingface.co` or `https://www.kaggle.com` over HTTPS. If a CDN is compromised, a MITM proxy is present, or the URL is attacker-controlled, the content is arbitrary.

**Existing controls:** TLS transport security only. No checksum, hash, or digital signature verification.

**Attack path:** A poisoned model file (adversarial weights, embedded payloads, or a file that exploits a parser vulnerability in LiteRT) is downloaded and loaded into the inference engine without any integrity check. No SHA256 hash comparison, no GPG signature verification, no reproducible build attestation.

---

## Finding 8: Model Download URL Constructed from User-Controlled Model Name (Limited Path Traversal)

**File:** `acdp-llm/src/model_management.rs` lines 348–357
**Severity:** MEDIUM

```rust
let download_url = if model_name.starts_with("google/") {
    format!("https://www.kaggle.com/models/{}/download", model_name)
} else {
    format!(
        "https://huggingface.co/{}/resolve/main/model.gguf",
        model_name
    )
};
```

**Attacker-controlled input:** `model_name` can come from user input or configuration.

**Existing controls:** The `safe_name = model_name.replace('/', "_")` on line 366 sanitizes the local filesystem path. However, the URL itself is not validated — a model_name like `../../some/other/path` or `evil.example.com/../` could redirect the download to unintended URLs.

**Attack path:** An attacker who controls the model name (e.g., through config manipulation or API input) could cause the system to download from an arbitrary URL path on HuggingFace or Kaggle, or cause confusing file storage paths. The risk is limited by HTTPS hostname pinning but worth noting.

---

## Finding 9: ACME Fallback Contact Email Uses Unauthenticated Default

**File:** `acdp-tui/src/http_server.rs` lines 633–636
**Severity:** LOW

```rust
let contacts: Vec<String> = if let Some(email) = &tls_config.email {
    vec![format!("mailto:{}", email)]
} else {
    vec!["mailto:admin@example.com".to_string()]
};
```

**Attacker-controlled input:** No — this is configuration-driven.

**Existing controls:** None.

**Attack path:** If no email is configured, the system registers with Let's Encrypt using `admin@example.com`, which is an uncontrolled domain. Let's Encrypt sends revocation and expiry notices to this address. An attacker controlling `example.com` could receive certificate notifications and potentially social-engineer revocation. Additionally, this contact is then double-prefixed: the code later applies `format!("mailto:{}", e)` to already-prefixed contacts (line 647), producing `mailto:mailto:admin@example.com`.

---

## Finding 10: DNS Credentials Stored in Configuration File as Plaintext

**File:** `acdp-llm/src/config.rs` lines 289–291
**Severity:** MEDIUM

```rust
/// DNS provider credentials (JSON string or path to file)
#[serde(default)]
pub dns_credentials: Option<String>,
```

**Attacker-controlled input:** No — configuration file.

**Existing controls:** The field supports "path to file" as an alternative, but the code never distinguishes between inline JSON credentials and a file path. The credentials are serialized to TOML config files and could be inadvertently committed to version control.

**Attack path:** If DNS credentials (e.g., Cloudflare API tokens for DNS-01 ACME challenges) are stored inline in the config file, they persist in plaintext on disk. Any process with read access to the config file gains DNS zone control, enabling domain takeover or certificate misissuance.

---

## Finding 11: `AuthConfig` Stores OAuth Client Secret and Bearer Tokens in Serializable Config

**File:** `acdp-core/src/transport/config.rs` lines 497–517
**Severity:** MEDIUM

```rust
pub enum AuthConfig {
    Basic { username: String, password: String },
    Bearer { token: String },
    OAuth {
        client_id: String,
        client_secret: String,
        token_url: Url,
        scope: Option<String>,
    },
    Header { name: String, value: String },
}
```

**Attacker-controlled input:** No — this is configuration.

**Existing controls:** None. The struct derives `Serialize` and `Deserialize`, and `to_file()` on line 207 writes configs to disk.

**Attack path:** Sensitive credentials (OAuth client_secret, bearer tokens, passwords) are written to JSON/YAML/TOML config files in plaintext via `TransportConfig::to_file()`. No encryption, no redaction, no warning. Any file system access reveals secrets.

---

## Finding 12: `expand_path` Does Not Prevent Path Traversal

**File:** `acdp-llm/src/config.rs` lines 159–173
**Severity:** LOW

```rust
fn expand_path(path: &str) -> LlmResult<PathBuf> {
    if path.starts_with("~/") {
        let home = dirs::home_dir()...;
        Ok(home.join(&path[2..]))
    } else if path.starts_with('~') {
        let home = dirs::home_dir()...;
        Ok(home.join(&path[1..]))
    } else {
        Ok(PathBuf::from(path))
    }
}
```

**Attacker-controlled input:** Depends — if config is user-editable, paths like `../../etc/passwd` or `~/../../etc/shadow` could be supplied.

**Existing controls:** None. No canonicalization, no path component validation.

**Attack path:** A `cache_dir` or `database_path` config value of `../../sensitive/directory` would be used directly. The function does not canonicalize, check for `..` segments, or validate that the result stays within an expected subtree. This enables reading/writing outside intended directories if an attacker can influence config values.

---

## Finding 13: ARC Nonce Is Client-Supplied u64 Without Server-Side Nonce Management

**File:** `acdp-auth/src/arc.rs` lines 435–448, 565–570
**Severity:** MEDIUM

```rust
pub fn create_presentation(
    &self,
    presentation_context: &[u8],
    nonce: u64,
    generators: &ARCGenerators,
) -> Result<ARCPresentation> {
    // ...
}

// In verify():
if nonce >= presentation_limit {
    return Ok(false);
}
```

**Attacker-controlled input:** Yes — the nonce is supplied by the presenter (client).

**Existing controls:** The verifier checks `nonce < presentation_limit`, but there is no server-side nonce tracking to prevent replay.

**Attack path:** The ARC rate-limiting design requires the server to track which nonces have been used (to detect double-spending). Without a server-side nonce registry, a client can reuse the same nonce value across multiple presentations. The server-side `nonce < presentation_limit` check only validates range, not uniqueness. This completely undermines the rate-limiting guarantee of ARC.

---

## Finding 14: `ServerPrivateKey::to_bytes()` Exposes Key Material Without Protection

**File:** `acdp-auth/src/arc.rs` lines 94–102
**Severity:** MEDIUM

```rust
pub fn to_bytes(&self) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(128);
    bytes.extend_from_slice(&self.x0.to_bytes());
    bytes.extend_from_slice(&self.x1.to_bytes());
    bytes.extend_from_slice(&self.x2.to_bytes());
    bytes.extend_from_slice(&self.x0_blinding.to_bytes());
    bytes
}
```

**Attacker-controlled input:** No.

**Existing controls:** None. No zeroization on drop, no memory protection.

**Attack path:** The `Vec<u8>` containing the private key is subject to standard memory management — it may be paged to swap, remain in freed memory, or be visible in core dumps. The `ServerPrivateKey` struct does not implement `Drop` with zeroization (e.g., via the `zeroize` crate). This is particularly relevant because the key material is long-lived (it determines the entire ARC credential system's security).

---

## Finding 15: `ARCCredentialResponse` Issuance Proof Is a Fixed Zero Vector

**File:** `acdp-auth/src/arc.rs` line 344
**Severity:** INFORMATIONAL (noted as already known, including for completeness of the crypto analysis)

```rust
let proof = vec![0u8; 64]; // Placeholder
```

**Note:** This is listed as already known. The significance is that the issuance response contains no proof of correct computation, meaning a malicious server could issue malformed credentials that will fail during presentation or could be used to deanonymize users.

---

## Finding 16: HuggingFace Token Passed Without TLS Certificate Pinning

**File:** `acdp-llm/src/model_management.rs` lines 178–191
**Severity:** LOW

```rust
let response = client
    .get("https://huggingface.co/api/models")
    .query(&[("search", "gemma"), ("filter", "litert"), ("limit", "20")])
    .header("Authorization", format!("Bearer {}", token))
    .timeout(std::time::Duration::from_secs(5))
    .send()
    .await
```

**Attacker-controlled input:** No — token comes from environment variable `HF_TOKEN`.

**Existing controls:** HTTPS (TLS transport).

**Attack path:** Standard reqwest client without certificate pinning. In environments where a corporate proxy performs TLS interception (MITM), the HuggingFace API token leaks to the proxy. This is a common enterprise deployment concern. The token provides read/write access to the user's HuggingFace account.

---

## Finding 17: `.env.example` Contains Zero-Value Keys That Could Be Mistaken for Real Configuration

**File:** `acdp-gateway/.env.example` lines 17–18
**Severity:** LOW

```
ACDP_GATEWAY_SIGNING_KEY=0000000000000000000000000000000000000000000000000000000000000000
ACDP_GATEWAY_PUBLIC_KEY=0000000000000000000000000000000000000000000000000000000000000000
```

**Attack path:** If `.env.example` is copied to `.env` without regenerating keys, the gateway uses an all-zeros Ed25519 seed, which is a well-known weak key. Any attacker can derive the same keypair and forge credentials. The `from_env()` code does not validate that the key is non-zero or meets minimum entropy requirements.

---

## Summary Table

| # | Severity | Component | Finding |
|---|----------|-----------|---------|
| 1 | HIGH | acdp-auth/mcp.rs | JWT decode disables exp and aud validation |
| 2 | MEDIUM | acdp-auth/mcp.rs | JWT uses HMAC (HS256) instead of asymmetric signing |
| 3 | CRITICAL | acdp-gateway/credential_issue.rs | Production endpoint uses hardcoded `test-secret` |
| 4 | HIGH | acdp-gateway/rauthy_client.rs | Client validation always returns true |
| 5 | HIGH | acdp-gateway/credential.rs | Gateway signing data excludes principal/agent/capabilities |
| 6 | HIGH | acdp-gateway/credential.rs vs acdp-auth/credentials.rs | Signing data mismatch between issuance and verification |
| 7 | HIGH | acdp-llm/model_management.rs | No integrity verification for downloaded models |
| 8 | MEDIUM | acdp-llm/model_management.rs | URL constructed from unvalidated model name |
| 9 | LOW | acdp-tui/http_server.rs | ACME uses unauthenticated fallback email |
| 10 | MEDIUM | acdp-llm/config.rs | DNS credentials stored as plaintext in config |
| 11 | MEDIUM | acdp-core/transport/config.rs | OAuth secrets/tokens serialized to disk in plaintext |
| 12 | LOW | acdp-llm/config.rs | Path expansion allows traversal |
| 13 | MEDIUM | acdp-auth/arc.rs | No server-side nonce tracking for ARC replay prevention |
| 14 | MEDIUM | acdp-auth/arc.rs | Private key material not zeroized on drop |
| 15 | INFO | acdp-auth/arc.rs | Issuance proof is stub (already known) |
| 16 | LOW | acdp-llm/model_management.rs | HF token sent without certificate pinning |
| 17 | LOW | acdp-gateway/.env.example | Zero-value keys in example config |
