# mcp-auth: Agent Credential Delegation Protocol (ACDP)

ACDP implementation for MCP (Model Context Protocol) authentication based on the [ACDP Protocol Spec v0.3](/Users/rpm/peer-id/peer-federate/ACDP_PROTOCOL_SPEC_V3.md).

## Overview

mcp-auth provides a complete implementation of ACDP for authenticating autonomous agents accessing MCP tools with:

- **Identity-Bound Credentials**: Full accountability chain from human principal to agent
- **Anonymous Rate-Limited Credentials**: ARC-based privacy-preserving auth
- **Hybrid Credentials**: Enterprise control + user privacy
- **MCP Integration**: Enterprise-Managed Authorization (ID-JAG flow)
- **Peer Delegation**: Agent A → Agent B with capability reduction
- **Cryptographic Rate Limiting**: ARC-based presentation counting

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│           HUMAN PRINCIPAL (Alice)                       │
│  Authenticates via WebAuthn/Passkey or Enterprise SSO   │
└─────────────────────────────────────────────────────────┘
                      ↓
        ┌─────────────────────────────┐
        │   ENTERPRISE IdP            │
        │  Issues: ID Token (OIDC)    │
        └─────────────────────────────┘
                      ↓
        ┌─────────────────────────────┐
        │   ACDP GATEWAY              │
        │   Token Exchange:           │
        │   ID Token → ID-JAG         │
        │   ID-JAG → ACDP Credential  │
        └─────────────────────────────┘
                      ↓
        ┌─────────────────────────────┐
        │   AGENT                     │
        │   Presents credential       │
        └─────────────────────────────┘
                      ↓
        ┌─────────────────────────────┐
        │   MCP SERVER                │
        │   Verifies credential       │
        └─────────────────────────────┘
```

## Modules

### Core Types

- **`credentials.rs`**: Three credential types (Identity-Bound, Anonymous, Hybrid)
- **`principal.rs`**: Human principal from ID-JAG
- **`agent.rs`**: Agent identity and verification
- **`capabilities.rs`**: MCP tool access control and rate limiting
- **`delegation.rs`**: Agent-to-agent delegation with capability reduction
- **`arc.rs`**: Anonymous Rate-Limited Credentials implementation
- **`mcp.rs`**: MCP Enterprise-Managed Authorization (ID-JAG flow)
- **`verification.rs`**: Credential verification logic
- **`gateway.rs`**: ACDP Gateway API endpoints
- **`error.rs`**: ACDP error types

## Status

**✅ Foundation Complete**

- [x] Core credential types defined
- [x] Principal and agent modules
- [x] Capabilities and rate limiting
- [x] Delegation with capability reduction
- [x] ARC (placeholder - ZK proofs stubbed)
- [x] ID-JAG token exchange
- [x] Credential verification framework
- [x] Gateway skeleton with HTTP endpoints

**🚧 In Progress**

- [ ] Full ARC cryptographic implementation (Schnorr ZK proofs, algebraic MACs)
- [ ] Database integration (replacing hiqlite due to dependency conflict)
- [ ] Complete gateway API endpoints
- [ ] Integration with Rauthy OAuth/OIDC server
- [ ] Tests for all modules
- [ ] Documentation and examples

**📋 Planned**

- [ ] SOC2 Type I certification path
- [ ] Multi-tenancy support
- [ ] HSM integration for key storage
- [ ] Kubernetes deployment
- [ ] MCP Registry integration
- [ ] IETF submission (draft-kontext-acdp-00)

## Usage

### Running the Gateway

```bash
cargo run --bin acdp-gateway
```

The gateway starts on `http://0.0.0.0:8080` with endpoints:

- `GET /health` - Health check
- `POST /acdp/v1/credentials/issue` - Issue credentials (TODO)
- `POST /acdp/v1/verify` - Verify credentials (TODO)

### As a Library

```rust
use mcp_auth::{
    credentials::{ACDPCredential, CredentialType},
    gateway::ACDPGateway,
    mcp::IDJAGToken,
};

// Create gateway
let gateway = ACDPGateway::new("https://acdp-gateway.kontext.dev/".to_string());

// Issue credential
let id_jag = IDJAGToken::new(/* ... */)?;
let request = CredentialIssuanceRequest {
    agent_id: "agent://claude-assistant".to_string(),
    credential_type: CredentialType::IdentityBound,
    // ...
};

let response = gateway.issue_credential(&id_jag, &request).await?;
```

## Dependencies

- **Cryptography**: `ed25519-compact`, `ring`, `chacha20poly1305`
- **JWT**: `jsonwebtoken`
- **Web**: `actix-web` (matching Rauthy stack)
- **Async**: `tokio`
- **Serialization**: `serde`, `serde_json`
- **Database**: TBD (hiqlite removed due to conflict)

## Development

```bash
# Build library and binary
cargo build -p mcp-auth

# Run tests
cargo test -p mcp-auth

# Run with logging
RUST_LOG=info cargo run --bin acdp-gateway
```

## Notes

- **Database**: Removed hiqlite dependency due to libsqlite3-sys version conflict with mcp-llm. Will use alternative (Redis, PostgreSQL, or in-memory).
- **ARC Implementation**: Currently uses placeholder ZK proofs. Full Schnorr proof implementation planned.
- **Rauthy Integration**: Forked Rauthy to `~/rauthy` for OAuth/OIDC base functionality.

## License

Apache-2.0

## References

- [ACDP Protocol Spec v0.3](/Users/rpm/peer-id/peer-federate/ACDP_PROTOCOL_SPEC_V3.md)
- [MCP Specification](https://modelcontextprotocol.io/)
- [Privacy Pass ARC](https://datatracker.ietf.org/doc/draft-privacypass-rate-limit-tokens/)
- [OAuth 2.0 Token Exchange (RFC 8693)](https://datatracker.ietf.org/doc/html/rfc8693)
- [Rauthy](https://github.com/sebadob/rauthy)
