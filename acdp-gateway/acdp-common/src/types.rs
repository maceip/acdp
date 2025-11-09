//! Common types for ACDP Gateway

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

/// Credential issuance request
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct CredentialIssuanceRequest {
    /// Agent identifier (e.g., "agent://claude-assistant")
    #[validate(length(min = 1))]
    pub agent_id: String,

    /// Agent public key (Ed25519, hex-encoded)
    #[validate(length(min = 64, max = 64))]
    pub agent_public_key: String,

    /// Credential type
    pub credential_type: CredentialType,

    /// Capabilities
    pub capabilities: CredentialCapabilities,

    /// Credential duration in days
    #[validate(range(min = 1, max = 365))]
    pub duration_days: u32,
}

/// Credential type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialType {
    /// Identity-bound credential (full audit trail)
    IdentityBound,

    /// Anonymous credential (ARC-based)
    Anonymous,

    /// Hybrid credential (enterprise control + user privacy)
    Hybrid,
}

/// Credential capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct CredentialCapabilities {
    /// Rate limiting configuration
    pub rate_limit: RateLimitConfig,

    /// MCP tool access control
    pub mcp_tools: MCPToolAccess,
}

/// Rate limit configuration
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct RateLimitConfig {
    /// Maximum presentations allowed
    #[validate(range(min = 1, max = 1000000))]
    pub max_presentations: u64,

    /// Time window (e.g., "24h", "7d")
    #[validate(length(min = 2, max = 10))]
    pub window: String,
}

/// MCP tool access control
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPToolAccess {
    /// Allowed MCP tools (e.g., ["filesystem/read_file", "filesystem/write_file"])
    pub allowed: Vec<String>,

    /// Denied MCP tools (takes precedence over allowed)
    #[serde(default)]
    pub denied: Vec<String>,
}

/// Credential verification request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialVerificationRequest {
    /// Credential ID
    pub credential_id: Uuid,

    /// Presentation context (MCP server, tool name, etc.)
    pub presentation_context: String,

    /// Nonce for presentation
    pub nonce: u64,

    /// Serialized credential
    pub credential: String,

    /// Optional ARC presentation (for anonymous/hybrid credentials)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arc_presentation: Option<String>,
}

/// Credential verification result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialVerificationResult {
    /// Whether credential is valid
    pub valid: bool,

    /// Human principal (if identity-bound)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub principal: Option<PrincipalInfo>,

    /// Agent ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,

    /// Presentations remaining
    pub presentations_remaining: u64,

    /// Delegation chain audit trail
    pub delegation_chain: Vec<String>,

    /// Reason for failure (if invalid)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,

    /// Timestamp of verification
    pub verified_at: DateTime<Utc>,
}

/// Principal information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrincipalInfo {
    /// Subject (e.g., "alice@acme.com")
    pub subject: String,

    /// Issuer (IdP URL)
    pub issuer: String,

    /// Client ID
    pub client_id: String,
}

/// Delegation request
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct DelegationRequest {
    /// Parent credential ID
    pub parent_credential_id: Uuid,

    /// Child agent ID
    #[validate(length(min = 1))]
    pub child_agent_id: String,

    /// Child agent public key
    #[validate(length(min = 64, max = 64))]
    pub child_agent_public_key: String,

    /// Delegated capabilities (subset of parent)
    pub capabilities: CredentialCapabilities,

    /// Duration in days
    #[validate(range(min = 1, max = 365))]
    pub duration_days: u32,
}

/// Stored credential metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCredential {
    /// Credential ID
    pub credential_id: Uuid,

    /// Credential type
    pub credential_type: CredentialType,

    /// Principal subject (for identity-bound/hybrid)
    pub principal_subject: Option<String>,

    /// Principal issuer (for identity-bound/hybrid)
    pub principal_issuer: Option<String>,

    /// Agent ID
    pub agent_id: String,

    /// Serialized credential (JSON)
    pub credential_data: String,

    /// Maximum presentations
    pub max_presentations: u64,

    /// Presentations used
    pub presentations_used: u64,

    /// Issued at
    pub issued_at: DateTime<Utc>,

    /// Expires at
    pub expires_at: DateTime<Utc>,

    /// Parent credential ID (for delegated credentials)
    pub parent_credential_id: Option<Uuid>,

    /// Revoked flag
    pub revoked: bool,
}
