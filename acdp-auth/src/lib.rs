//! # mcp-auth: Agent Credential Delegation Protocol (ACDP) Implementation
//!
//! This crate implements the ACDP v0.3 specification for MCP (Model Context Protocol) authentication.
//!
//! ## Features
//!
//! - **Identity-Bound Credentials**: Full accountability chain from human principal to agent
//! - **Anonymous Rate-Limited Credentials**: ARC-based privacy-preserving auth
//! - **Hybrid Credentials**: Enterprise control + user privacy
//! - **MCP Integration**: Enterprise-Managed Authorization (ID-JAG flow)
//! - **Peer Delegation**: Agent A → Agent B with capability reduction
//! - **Cryptographic Rate Limiting**: ARC-based presentation counting
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │           HUMAN PRINCIPAL (Alice)                       │
//! │  Authenticates via WebAuthn/Passkey or Enterprise SSO   │
//! └─────────────────────────────────────────────────────────┘
//!                       ↓
//!         ┌─────────────────────────────┐
//!         │   ENTERPRISE IdP            │
//!         │  Issues: ID Token (OIDC)    │
//!         └─────────────────────────────┘
//!                       ↓
//!         ┌─────────────────────────────┐
//!         │   ACDP GATEWAY              │
//!         │   Token Exchange:           │
//!         │   ID Token → ID-JAG         │
//!         │   ID-JAG → ACDP Credential  │
//!         └─────────────────────────────┘
//!                       ↓
//!         ┌─────────────────────────────┐
//!         │   AGENT                     │
//!         │   Presents credential       │
//!         └─────────────────────────────┘
//!                       ↓
//!         ┌─────────────────────────────┐
//!         │   MCP SERVER                │
//!         │   Verifies credential       │
//!         └─────────────────────────────┘
//! ```

#![warn(missing_docs)]

pub mod agent;
pub mod arc;
pub mod arc_zkp;
pub mod capabilities;
pub mod credentials;
pub mod delegation;
pub mod error;
pub mod gateway;
pub mod mcp;
pub mod principal;
pub mod verification;

// Re-exports for convenience
pub use agent::Agent;
pub use arc::{
    ARCCredential, ARCCredentialRequest, ARCCredentialResponse, ARCGenerators, ARCPresentation,
    ARCProof, ClientSecrets, ServerPrivateKey, ServerPublicKey,
};
pub use capabilities::{MCPCapabilities, RateLimitParams, ResourceLimits};
pub use credentials::{
    ACDPCredential, AnonymousCredential, CredentialType, HybridCredential, IdentityBoundCredential,
};
pub use delegation::{DelegationChain, DelegationProof, DelegationRights};
pub use error::{ACDPError, Result};
pub use mcp::{IDJAGToken, TokenExchangeRequest};
pub use principal::Principal;

/// ACDP Protocol version
pub const ACDP_VERSION: &str = "ACDP/0.3";

/// Default credential duration (7 days)
pub const DEFAULT_CREDENTIAL_DURATION_DAYS: u64 = 7;

/// Default rate limit window (24 hours)
pub const DEFAULT_RATE_LIMIT_WINDOW_HOURS: u64 = 24;

/// Maximum delegation depth to prevent infinite chains
pub const MAX_DELEGATION_DEPTH: u8 = 5;
