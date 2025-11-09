# ACDP Gateway Interfaces

Complete interface documentation for all APIs, types, and integration points.

---

## 1. HTTP REST API

### 1.1 Health Check

**Endpoint**: `GET /health`

**Response**:
```json
{
  "status": "healthy",
  "version": "0.1.0"
}
```

---

### 1.2 Issue Credential

**Endpoint**: `POST /acdp/v1/credentials/issue`

**Authentication**: Bearer token (ID-JAG)

**Request Headers**:
```
Authorization: Bearer <ID-JAG-JWT>
Content-Type: application/json
```

**Request Body**:
```typescript
{
  agent_id: string,              // e.g., "agent://claude-assistant"
  agent_public_key: string,      // 64-char hex Ed25519 public key
  credential_type: "identity_bound" | "anonymous" | "hybrid",
  capabilities: {
    rate_limit: {
      max_presentations: number,  // 1-1000000
      window: string              // e.g., "24h", "7d"
    },
    mcp_tools: {
      allowed: string[],          // e.g., ["filesystem/read_file"]
      denied: string[]            // optional, takes precedence
    }
  },
  duration_days: number           // 1-365
}
```

**Response** (200 OK):
```typescript
{
  credential: ACDPCredential,     // Full credential object (see section 2.1)
  credential_id: string,          // UUID
  credential_type: number         // 0=IdentityBound, 1=Anonymous, 2=Hybrid
}
```

**Errors**:
- `401 Unauthorized` - Missing/invalid ID-JAG token
- `400 Bad Request` - Invalid request body
- `500 Internal Server Error` - Credential issuance failed

**Example**:
```bash
curl -X POST http://localhost:8080/acdp/v1/credentials/issue \
  -H "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6Im9hdXRoLWlkLWphZytqd3QifQ..." \
  -H "Content-Type: application/json" \
  -d '{
    "agent_id": "agent://claude-assistant",
    "agent_public_key": "a1b2c3d4...",
    "credential_type": "identity_bound",
    "capabilities": {
      "rate_limit": {
        "max_presentations": 1000,
        "window": "24h"
      },
      "mcp_tools": {
        "allowed": ["filesystem/read_file", "filesystem/write_file"],
        "denied": []
      }
    },
    "duration_days": 7
  }'
```

---

### 1.3 Verify Credential

**Endpoint**: `POST /acdp/v1/credentials/verify`

**Request Body**:
```typescript
{
  credential_id: string,          // UUID
  presentation_context: string,   // e.g., "mcp://server.example.com/filesystem/read_file"
  nonce: number,                  // Presentation nonce (0-999)
  credential: string,             // JSON-serialized ACDPCredential
  arc_presentation?: string       // Optional, for anonymous/hybrid credentials
}
```

**Response** (200 OK):
```typescript
{
  valid: boolean,
  principal?: {                   // Only for identity-bound credentials
    subject: string,              // e.g., "alice@acme.com"
    issuer: string,               // e.g., "https://acme.idp.example"
    client_id: string             // e.g., "mcp-client"
  },
  agent_id?: string,              // Only for identity-bound credentials
  presentations_remaining: number,
  delegation_chain: string[],     // e.g., ["agent://a → agent://b"]
  failure_reason?: string,        // Only if valid=false
  verified_at: string             // ISO 8601 timestamp
}
```

**Example**:
```bash
curl -X POST http://localhost:8080/acdp/v1/credentials/verify \
  -H "Content-Type: application/json" \
  -d '{
    "credential_id": "550e8400-e29b-41d4-a716-446655440000",
    "presentation_context": "mcp://server.example.com/tool",
    "nonce": 42,
    "credential": "{\"type\":\"identity_bound\",...}"
  }'
```

---

### 1.4 Delegate Credential

**Endpoint**: `POST /acdp/v1/credentials/delegate`

**Request Body**:
```typescript
{
  parent_credential_id: string,       // UUID of parent credential
  child_agent_id: string,             // e.g., "agent://sub-agent"
  child_agent_public_key: string,     // 64-char hex Ed25519 public key
  capabilities: {                     // Must be subset of parent
    rate_limit: {
      max_presentations: number,
      window: string
    },
    mcp_tools: {
      allowed: string[],
      denied: string[]
    }
  },
  duration_days: number               // 1-365, must be <= parent remaining
}
```

**Response** (200 OK):
```typescript
{
  credential: ACDPCredential,
  credential_id: string,
  parent_credential_id: string
}
```

**Status**: ⚠️ Not yet implemented (stub returns 500)

---

## 2. Data Structures

### 2.1 ACDPCredential (from mcp-auth)

**Rust Definition**:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ACDPCredential {
    IdentityBound(IdentityBoundCredential),
    Anonymous(AnonymousCredential),
    Hybrid(HybridCredential),
}
```

#### 2.1.1 IdentityBoundCredential

```rust
pub struct IdentityBoundCredential {
    pub version: String,                    // "0.3"
    pub credential_id: Uuid,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub principal: Principal,               // See 2.2
    pub agent: Agent,                       // See 2.3
    pub mcp_capabilities: MCPCapabilities,  // See 2.4
    pub delegation: DelegationRights,       // See 2.5
    pub delegation_chain: DelegationChain,  // See 2.6
    pub signature: Signature,               // Ed25519
    pub extensions: Extensions,
}
```

**JSON Example**:
```json
{
  "type": "identity_bound",
  "version": "0.3",
  "credential_id": "550e8400-e29b-41d4-a716-446655440000",
  "issued_at": "2025-11-09T12:00:00Z",
  "expires_at": "2025-11-16T12:00:00Z",
  "principal": {
    "subject": "alice@acme.com",
    "issuer": "https://acme.idp.example",
    "client_id": "mcp-client"
  },
  "agent": {
    "agent_id": "agent://claude-assistant",
    "public_key": [161, 178, 195, ...],
    "agent_type": "mcp-client"
  },
  "mcp_capabilities": {
    "allowed_tools": ["filesystem/read_file", "filesystem/write_file"],
    "denied_tools": []
  },
  "delegation": {
    "can_delegate": false,
    "max_depth": 0,
    "allowed_capabilities": []
  },
  "delegation_chain": {...},
  "signature": "a1b2c3...",
  "extensions": {}
}
```

#### 2.1.2 AnonymousCredential

```rust
pub struct AnonymousCredential {
    pub version: String,
    pub credential_id: Uuid,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub arc_credential: ARCCredential,      // See 2.7
    pub mcp_capabilities: MCPCapabilities,
    pub extensions: Extensions,
}
```

**Key Difference**: No `principal` or `agent` fields. Identity hidden from tool provider.

#### 2.1.3 HybridCredential

```rust
pub struct HybridCredential {
    pub version: String,
    pub credential_id: Uuid,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub principal: Principal,               // Known to gateway only
    pub agent: Agent,                       // Known to gateway only
    pub arc_credential: ARCCredential,      // Presented to tool provider
    pub mcp_capabilities: MCPCapabilities,
    pub delegation: DelegationRights,
    pub delegation_chain: DelegationChain,
    pub signature: Signature,
    pub extensions: Extensions,
}
```

**Key Feature**: Gateway sees identity, tool provider sees only ARC presentation.

---

### 2.2 Principal

```rust
pub struct Principal {
    pub subject: String,        // User identifier (e.g., email)
    pub issuer: String,         // IdP URL
    pub client_id: String,      // OAuth client ID
    pub metadata: Option<Map<String, Value>>,
}
```

**Example**:
```json
{
  "subject": "alice@acme.com",
  "issuer": "https://acme.idp.example",
  "client_id": "mcp-client",
  "metadata": null
}
```

---

### 2.3 Agent

```rust
pub struct Agent {
    pub agent_id: String,           // URI, e.g., "agent://claude-assistant"
    pub public_key: Vec<u8>,        // Ed25519 public key (32 bytes)
    pub agent_type: String,         // e.g., "mcp-client"
    pub metadata: Option<Map<String, Value>>,
}
```

---

### 2.4 MCPCapabilities

```rust
pub struct MCPCapabilities {
    pub allowed_tools: Vec<String>,
    pub denied_tools: Vec<String>,
    pub max_calls_per_tool: Option<u64>,
    pub rate_limit_window: Option<String>,
}
```

**Example**:
```json
{
  "allowed_tools": ["filesystem/read_file", "filesystem/write_file"],
  "denied_tools": ["filesystem/execute"],
  "max_calls_per_tool": null,
  "rate_limit_window": "24h"
}
```

---

### 2.5 DelegationRights

```rust
pub struct DelegationRights {
    pub can_delegate: bool,
    pub max_depth: u32,
    pub allowed_capabilities: Vec<String>,
}
```

---

### 2.6 DelegationChain

```rust
pub struct DelegationChain {
    pub chain: Vec<DelegationEntry>,
}

pub struct DelegationEntry {
    pub delegator: String,      // Agent ID
    pub delegatee: String,      // Agent ID
    pub timestamp: DateTime<Utc>,
    pub signature: Vec<u8>,
}
```

**Audit Trail Method**:
```rust
impl DelegationChain {
    pub fn audit_trail(&self) -> Vec<String> {
        // Returns: ["agent://a → agent://b", "agent://b → agent://c"]
    }
}
```

---

### 2.7 ARCCredential (from mcp-auth)

```rust
pub struct ARCCredential {
    pub m1: Scalar,                 // Client attribute (hidden)
    pub u: ProjectivePoint,         // MAC component U
    pub u_prime: ProjectivePoint,   // MAC component Q
    pub x1: ProjectivePoint,        // Server public key component
    pub max_presentations: u64,
    pub presentations_used: AtomicU64,
}
```

**CMZ14 MACGGM Structure**:
- MAC Relation: `Q = (x0 + m1*x1 + m2*x2)*U`
- Presentation: Randomize `(U, Q)` by factor `a`, add ZK proof

---

### 2.8 ARCPresentation

```rust
pub struct ARCPresentation {
    pub u: ProjectivePoint,             // Randomized U
    pub u_prime_commit: ProjectivePoint,// Q + r*G
    pub m1_commit: ProjectivePoint,     // m1*U + z*G
    pub tag: ProjectivePoint,           // For double-spending prevention
    pub m1_tag: ProjectivePoint,
    pub t: ProjectivePoint,
    pub proof: ARCProof,                // ZK proof (sigma-proofs)
}
```

**ZK Proof Constraints**:
1. `m1Commit = m1 * U + z * G`
2. `V = z * X1 - r * G`
3. `T = (m1 + nonce) * tag`
4. `m1Tag = m1 * tag`

---

## 3. ID-JAG Token Format

### 3.1 ID-JAG Claims

**JWT Header**:
```json
{
  "alg": "RS256",  // or HS256, ES256
  "typ": "oauth-id-jag+jwt"
}
```

**JWT Claims**:
```typescript
{
  typ: "oauth-id-jag+jwt",        // Token type (MUST match)
  jti: string,                     // Unique JWT ID
  iss: string,                     // Issuer (IdP URL)
  sub: string,                     // Subject (user identifier)
  aud: string,                     // Audience (ACDP Gateway URL)
  resource: string,                // MCP Server URL
  client_id: string,               // OAuth client ID
  exp: number,                     // Expiration (Unix timestamp)
  iat: number,                     // Issued at (Unix timestamp)
  scope: string                    // Space-separated MCP scopes
}
```

**Example**:
```json
{
  "typ": "oauth-id-jag+jwt",
  "jti": "9e43f81b64a33f20116179",
  "iss": "https://acme.idp.example",
  "sub": "alice@acme.com",
  "aud": "https://acdp-gateway.kontext.dev/",
  "resource": "https://mcp-server.example.com/",
  "client_id": "mcp-client",
  "exp": 1731281970,
  "iat": 1731280970,
  "scope": "mcp:filesystem:read mcp:filesystem:write"
}
```

---

## 4. Database Schema

### 4.1 acdp_credentials Table

**PostgreSQL Schema**:
```sql
CREATE TABLE acdp_credentials (
    credential_id UUID PRIMARY KEY,
    credential_type INTEGER NOT NULL,           -- 0=IdentityBound, 1=Anonymous, 2=Hybrid
    principal_subject TEXT,                     -- NULL for anonymous
    principal_issuer TEXT,                      -- NULL for anonymous
    agent_id TEXT NOT NULL,
    credential_data TEXT NOT NULL,              -- JSON serialized ACDPCredential
    max_presentations BIGINT NOT NULL,
    presentations_used BIGINT NOT NULL DEFAULT 0,
    issued_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    parent_credential_id UUID REFERENCES acdp_credentials(credential_id),
    revoked BOOLEAN NOT NULL DEFAULT false,
    revoked_at TIMESTAMPTZ,
    revocation_reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

**Indexes**:
- `idx_acdp_credentials_agent_id` on `agent_id`
- `idx_acdp_credentials_principal` on `(principal_subject, principal_issuer)`
- `idx_acdp_credentials_expires_at` on `expires_at`
- `idx_acdp_credentials_parent` on `parent_credential_id`

---

## 5. Rauthy Integration Points

### 5.1 Token Exchange Endpoint (TO BE ADDED)

**Endpoint**: `POST /oauth2/token` (in Rauthy)

**Request**:
```
Content-Type: application/x-www-form-urlencoded

grant_type=urn:ietf:params:oauth:grant-type:token-exchange
&requested_token_type=urn:ietf:params:oauth:token-type:id-jag
&audience=https://acdp-gateway.kontext.dev/
&resource=https://mcp-server.example.com/
&scope=mcp:filesystem:read+mcp:filesystem:write
&subject_token=<ID-TOKEN>
&subject_token_type=urn:ietf:params:oauth:token-type:id_token
&client_id=mcp-client
&client_secret=<secret>
```

**Response**:
```json
{
  "issued_token_type": "urn:ietf:params:oauth:token-type:id-jag",
  "access_token": "<ID-JAG-JWT>",
  "token_type": "N_A",
  "scope": "mcp:filesystem:read mcp:filesystem:write",
  "expires_in": 300
}
```

---

### 5.2 UserInfo Endpoint (EXISTING)

**Endpoint**: `GET /oidc/userinfo` (in Rauthy)

**Used By**: `RauthyClient::get_user_info()`

**Response**:
```json
{
  "sub": "user-uuid",
  "email": "alice@acme.com",
  "email_verified": true,
  "name": "Alice Smith"
}
```

---

## 6. Service Interfaces (Rust Traits/Structs)

### 6.1 CredentialService

```rust
impl CredentialService {
    pub fn new(
        db_pool: PgPool,
        signing_key: Vec<u8>,
        public_key: Vec<u8>,
        gateway_issuer: String,
    ) -> Self;

    pub async fn issue_credential(
        &self,
        principal: Principal,
        request: CredentialIssuanceRequest,
    ) -> Result<ACDPCredential>;

    // Internal methods
    fn issue_identity_bound(...) -> Result<ACDPCredential>;
    async fn issue_anonymous(...) -> Result<ACDPCredential>;
    async fn issue_hybrid(...) -> Result<ACDPCredential>;
}
```

---

### 6.2 RauthyClient

```rust
impl RauthyClient {
    pub fn new(base_url: String, admin_token: String) -> Self;

    pub async fn verify_id_token(&self, id_token: &str)
        -> Result<IDTokenClaims>;

    pub async fn get_user_info(&self, access_token: &str)
        -> Result<UserInfo>;

    pub async fn validate_client(&self, client_id: &str, client_secret: &str)
        -> Result<bool>;
}
```

---

### 6.3 ID-JAG Validation

```rust
pub fn validate_id_jag(
    token: &str,
    expected_audience: &str,
    idp_public_key: &DecodingKey,
) -> Result<IDJAGClaims>;
```

---

## 7. Configuration Interface

### 7.1 Environment Variables

| Variable | Type | Required | Description |
|----------|------|----------|-------------|
| `ACDP_SERVER_HOST` | String | No | Server bind address (default: 127.0.0.1) |
| `ACDP_SERVER_PORT` | u16 | No | Server port (default: 8080) |
| `DATABASE_URL` | String | Yes | PostgreSQL connection string |
| `RAUTHY_BASE_URL` | String | No | Rauthy URL (default: http://localhost:8000) |
| `RAUTHY_ADMIN_TOKEN` | String | Yes | Rauthy admin API token |
| `ACDP_GATEWAY_ISSUER` | String | No | Gateway issuer URL |
| `ACDP_GATEWAY_SIGNING_KEY` | String | Yes | Hex-encoded Ed25519 secret key (64 chars) |
| `ACDP_GATEWAY_PUBLIC_KEY` | String | Yes | Hex-encoded Ed25519 public key (64 chars) |
| `RUST_LOG` | String | No | Logging level (default: info) |

---

## 8. Error Responses

### 8.1 Error Format

All errors return JSON:
```json
{
  "error": "Error message here"
}
```

### 8.2 HTTP Status Codes

| Code | Meaning | Example |
|------|---------|---------|
| 200 | Success | Credential issued |
| 400 | Bad Request | Invalid JSON, validation failed |
| 401 | Unauthorized | Missing/invalid ID-JAG token |
| 403 | Forbidden | Delegation not allowed |
| 429 | Too Many Requests | Rate limit exceeded |
| 500 | Internal Server Error | Database error, crypto error |

---

## 9. MCP Server Integration

### 9.1 How MCP Servers Verify Credentials

**Step 1**: Receive credential from MCP client

**Step 2**: Call ACDP Gateway verification endpoint
```bash
curl -X POST https://acdp-gateway.kontext.dev/acdp/v1/credentials/verify \
  -H "Content-Type: application/json" \
  -d '{
    "credential_id": "uuid",
    "presentation_context": "mcp://my-server.example.com/my_tool",
    "nonce": 42,
    "credential": "<serialized-credential>"
  }'
```

**Step 3**: Check response
```typescript
if (response.valid) {
  // Allow tool access
  // Log principal if identity-bound
  // Check presentations_remaining
} else {
  // Deny access
  // Log failure_reason
}
```

---

## 10. Testing Interface

### 10.1 Test Endpoints

**Health Check**:
```bash
curl http://localhost:8080/health
# Expected: {"status":"healthy","version":"0.1.0"}
```

**Issue Test Credential**:
```bash
# Generate test ID-JAG with test secret
jwt_token=$(node -e "console.log(require('jsonwebtoken').sign({...}, 'test-secret'))")

curl -X POST http://localhost:8080/acdp/v1/credentials/issue \
  -H "Authorization: Bearer $jwt_token" \
  -H "Content-Type: application/json" \
  -d @test_credential_request.json
```

---

## Summary

**Total Interfaces**:
- **4 HTTP endpoints** (health, issue, verify, delegate)
- **3 credential types** (identity-bound, anonymous, hybrid)
- **10+ data structures** (Principal, Agent, MCPCapabilities, etc.)
- **1 database table** with 5 indexes
- **3 Rust services** (CredentialService, RauthyClient, ID-JAG validator)
- **1 external integration** (Rauthy OIDC)

All interfaces follow ACDP v0.3 specification and are compatible with the mcp-auth cryptographic library.
