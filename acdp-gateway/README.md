# ACDP Gateway

**Agent Credential Delegation Protocol (ACDP) v0.3 Gateway**

Production-ready authentication gateway for autonomous agents using the Model Context Protocol (MCP).

## Overview

ACDP Gateway integrates:
- **Rauthy** - OIDC/OAuth2 Identity Provider (user authentication, ID tokens)
- **mcp-auth** - Cryptographic credential library (ARC, ZK proofs, signatures)
- **ACDP v0.3 Protocol** - Agent credential issuance, verification, delegation

## Architecture

```
User (WebAuthn/SSO)
        ↓
┌──────────────────────┐
│      **Rauthy**      │  OIDC/OAuth2 Identity Provider
│     (Port 8000)      │  • User authentication
└──────────┬───────────┘  • ID Token issuance
           │
           │ ID Token → ID-JAG exchange
           ↓
┌──────────────────────┐
│   **ACDP Gateway**   │  This Service (Actix-web)
│     (Port 8080)      │  • ID-JAG validation
└──────────┬───────────┘  • Credential issuance (3 types)
           │              • Credential verification
           │              • Rate limit enforcement
           ↓
┌──────────────────────┐
│    **mcp-auth**      │  Cryptographic Library
│    (Rust crate)      │  • ARC credentials (CMZ14 MACGGM)
└──────────────────────┘  • ZK proofs (sigma-proofs)
                          • Ed25519 signatures
                          • Delegation chains
```

**Flow**:
1. User authenticates with **Rauthy** (enterprise SSO)
2. **Rauthy** issues ID Token → ID-JAG (via token exchange)
3. MCP Client presents ID-JAG to **ACDP Gateway**
4. **ACDP Gateway** issues cryptographic credential using **mcp-auth**
5. MCP Client uses credential to access MCP Servers
6. MCP Server verifies credential with **ACDP Gateway**

## Features

### ✅ Implemented
- [x] ID-JAG token validation
- [x] Credential issuance (Identity-Bound, Anonymous, Hybrid)
- [x] ARC-based anonymous credentials with ZK proofs
- [x] CMZ14 MACGGM cryptographic rate limiting
- [x] Credential verification endpoint
- [x] PostgreSQL database schema
- [x] Rate limit enforcement

### 🚧 In Progress
- [ ] Agent delegation (Agent A → Agent B)
- [ ] Rauthy ID-JAG token exchange endpoint
- [ ] Dynamic IdP public key discovery (JWKS)

### 📋 Planned
- [ ] Credential revocation
- [ ] Audit log export
- [ ] Prometheus metrics
- [ ] MCP server SDK (Rust)

## Quick Start

### Prerequisites
- Rust 1.70+
- PostgreSQL 14+
- Rauthy running on `http://localhost:8000`

### 1. Setup Database

```bash
# Create database
createdb acdp_gateway

# Export connection string
export DATABASE_URL="postgresql://user:password@localhost/acdp_gateway"
```

### 2. Generate Gateway Keys

```bash
# Generate Ed25519 keypair for gateway
cargo run --bin generate-keys

# Output:
# ACDP_GATEWAY_SIGNING_KEY=<hex-encoded-secret-key>
# ACDP_GATEWAY_PUBLIC_KEY=<hex-encoded-public-key>
```

### 3. Configure Environment

Create `.env`:

```bash
# Server
ACDP_SERVER_HOST=127.0.0.1
ACDP_SERVER_PORT=8080

# Database
DATABASE_URL=postgresql://user:password@localhost/acdp_gateway

# Rauthy Integration
RAUTHY_BASE_URL=http://localhost:8000
RAUTHY_ADMIN_TOKEN=<your-rauthy-admin-token>

# Gateway Identity
ACDP_GATEWAY_ISSUER=https://acdp-gateway.kontext.dev/
ACDP_GATEWAY_SIGNING_KEY=<from-generate-keys>
ACDP_GATEWAY_PUBLIC_KEY=<from-generate-keys>

# Logging
RUST_LOG=info,acdp_server=debug
```

### 4. Run Migrations

```bash
cd acdp-server
sqlx migrate run
```

### 5. Start Server

```bash
cargo run --release
```

## API Endpoints

### Health Check

```bash
curl http://localhost:8080/health
```

### Issue Credential

```bash
curl -X POST http://localhost:8080/acdp/v1/credentials/issue \
  -H "Authorization: Bearer <ID-JAG-TOKEN>" \
  -H "Content-Type: application/json" \
  -d '{
    "agent_id": "agent://claude-assistant",
    "agent_public_key": "abcd1234...",
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

### Verify Credential

```bash
curl -X POST http://localhost:8080/acdp/v1/credentials/verify \
  -H "Content-Type: application/json" \
  -d '{
    "credential_id": "uuid",
    "presentation_context": "mcp://server.example.com/tool",
    "nonce": 42,
    "credential": "<serialized-credential>"
  }'
```

## Development

### Run Tests

```bash
cargo test
```

### Database Migrations

```bash
# Create new migration
sqlx migrate add <migration_name>

# Run migrations
sqlx migrate run

# Revert last migration
sqlx migrate revert
```

### Format & Lint

```bash
cargo fmt
cargo clippy
```

## Integration with Rauthy

To add ID-JAG token exchange to Rauthy, see [RAUTHY_INTEGRATION.md](./RAUTHY_INTEGRATION.md).

## Protocol Specification

See [ACDP_PROTOCOL_SPEC_V3.md](../peer-id/peer-federate/ACDP_PROTOCOL_SPEC_V3.md) for full protocol details.

## License

Apache-2.0
