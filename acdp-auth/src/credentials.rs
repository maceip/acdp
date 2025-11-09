//! ACDP Credential Types
//!
//! Implements three credential types from ACDP v0.3:
//! 1. Identity-Bound: Full audit trail (Human → Agent)
//! 2. Anonymous: ARC-based privacy-preserving credentials
//! 3. Hybrid: Enterprise control + user privacy

use crate::{
    agent::Agent,
    arc::{ARCCredential, ARCPresentation},
    capabilities::MCPCapabilities,
    delegation::{DelegationChain, DelegationRights},
    error::{ACDPError, Result},
    principal::Principal,
};
use chrono::{DateTime, Utc};
use ed25519_compact::Signature;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// ACDP Credential wrapper supporting all three types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ACDPCredential {
    /// Identity-bound credential with full audit trail
    IdentityBound(IdentityBoundCredential),

    /// Anonymous credential using ARC
    Anonymous(AnonymousCredential),

    /// Hybrid credential (enterprise control + user privacy)
    Hybrid(HybridCredential),
}

impl ACDPCredential {
    /// Get credential ID
    pub fn credential_id(&self) -> Uuid {
        match self {
            Self::IdentityBound(c) => c.credential_id,
            Self::Anonymous(c) => c.credential_id,
            Self::Hybrid(c) => c.credential_id,
        }
    }

    /// Check if credential is expired
    pub fn is_expired(&self) -> bool {
        match self {
            Self::IdentityBound(c) => c.expires_at < Utc::now(),
            Self::Anonymous(c) => c.expires_at < Utc::now(),
            Self::Hybrid(c) => c.expires_at < Utc::now(),
        }
    }

    /// Get MCP capabilities
    pub fn mcp_capabilities(&self) -> &MCPCapabilities {
        match self {
            Self::IdentityBound(c) => &c.mcp_capabilities,
            Self::Anonymous(c) => &c.mcp_capabilities,
            Self::Hybrid(c) => &c.mcp_capabilities,
        }
    }

    /// Get credential type name
    pub fn credential_type(&self) -> CredentialType {
        match self {
            Self::IdentityBound(_) => CredentialType::IdentityBound,
            Self::Anonymous(_) => CredentialType::Anonymous,
            Self::Hybrid(_) => CredentialType::Hybrid,
        }
    }

    /// Verify credential signature
    pub fn verify_signature(&self, issuer_public_key: &[u8]) -> Result<()> {
        match self {
            Self::IdentityBound(c) => c.verify_signature(issuer_public_key),
            Self::Anonymous(_) => {
                // Anonymous credentials don't have signatures - verification happens during presentation
                Ok(())
            }
            Self::Hybrid(c) => c.verify_signature(issuer_public_key),
        }
    }
}

/// Credential type enum for configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialType {
    /// Identity-bound credential
    IdentityBound,

    /// Anonymous ARC credential
    Anonymous,

    /// Hybrid credential
    Hybrid,
}

/// Identity-Bound Credential (Type 1)
///
/// Full accountability chain from human principal to agent.
/// Provides complete audit trail and delegation support.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityBoundCredential {
    /// Protocol version
    pub version: String,

    /// Unique credential identifier
    pub credential_id: Uuid,

    /// Issuance timestamp
    pub issued_at: DateTime<Utc>,

    /// Expiration timestamp
    pub expires_at: DateTime<Utc>,

    /// Human principal (from ID-JAG)
    pub principal: Principal,

    /// Agent identity
    pub agent: Agent,

    /// MCP tool access control
    pub mcp_capabilities: MCPCapabilities,

    /// Delegation rights
    pub delegation: DelegationRights,

    /// Delegation chain (audit trail)
    pub delegation_chain: DelegationChain,

    /// Gateway signature (Ed25519)
    #[serde(with = "signature_serde")]
    pub signature: Signature,

    /// Optional extensions
    #[serde(default)]
    pub extensions: Extensions,
}

/// Serde module for Signature serialization
mod signature_serde {
    use ed25519_compact::Signature;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(sig: &Signature, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let bytes = sig.as_ref();
        serializer.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Signature, D::Error>
    where
        D: Deserializer<'de>,
    {
        let hex_string = String::deserialize(deserializer)?;
        let bytes = hex::decode(&hex_string).map_err(serde::de::Error::custom)?;
        Signature::from_slice(&bytes).map_err(serde::de::Error::custom)
    }
}

impl IdentityBoundCredential {
    /// Verify gateway signature
    pub fn verify_signature(&self, issuer_public_key: &[u8]) -> Result<()> {
        let public_key = ed25519_compact::PublicKey::from_slice(issuer_public_key)
            .map_err(|e| ACDPError::CryptoError(format!("Invalid public key: {}", e)))?;

        // Create canonical representation for signing
        let signing_data = self.signing_data()?;

        public_key
            .verify(&signing_data, &self.signature)
            .map_err(|e| {
                ACDPError::InvalidCredential(format!("Signature verification failed: {}", e))
            })?;

        Ok(())
    }

    /// Get canonical signing data
    fn signing_data(&self) -> Result<Vec<u8>> {
        let mut data = Vec::new();

        // Deterministic serialization for signature verification
        data.extend_from_slice(self.version.as_bytes());
        data.extend_from_slice(self.credential_id.as_bytes());
        data.extend_from_slice(&self.issued_at.timestamp().to_le_bytes());
        data.extend_from_slice(&self.expires_at.timestamp().to_le_bytes());

        // Use serde_json for cross-platform compatibility
        let canonical = serde_json::to_vec(&(
            &self.principal,
            &self.agent,
            &self.mcp_capabilities,
            &self.delegation,
        ))
        .map_err(|e| ACDPError::InternalError(format!("Serialization failed: {}", e)))?;

        data.extend_from_slice(&canonical);

        Ok(data)
    }
}

/// Anonymous Credential (Type 2)
///
/// ARC-based privacy-preserving credentials.
/// No identity revealed to tool provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnonymousCredential {
    /// Protocol version
    pub version: String,

    /// Unique credential identifier (known only to client)
    pub credential_id: Uuid,

    /// Issuance timestamp
    pub issued_at: DateTime<Utc>,

    /// Expiration timestamp
    pub expires_at: DateTime<Utc>,

    /// ARC credential (algebraic MAC)
    pub arc_credential: ARCCredential,

    /// MCP tool access control
    pub mcp_capabilities: MCPCapabilities,

    /// Optional extensions
    #[serde(default)]
    pub extensions: Extensions,
}

impl AnonymousCredential {
    /// Create unlinkable presentation
    ///
    /// Generates a zero-knowledge proof of credential validity without revealing identity.
    pub fn create_presentation(
        &self,
        presentation_context: &[u8],
        nonce: u64,
    ) -> Result<ARCPresentation> {
        use crate::arc::ARCGenerators;
        let generators = ARCGenerators::new();
        self.arc_credential
            .create_presentation(presentation_context, nonce, &generators)
    }

    /// Check rate limit
    pub fn check_rate_limit(&self) -> Result<u64> {
        Ok(self.arc_credential.presentations_remaining())
    }
}

/// Hybrid Credential (Type 3)
///
/// Enterprise control + user privacy.
/// Enterprise IdP knows user, tool provider sees only anonymous credential.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridCredential {
    /// Protocol version
    pub version: String,

    /// Unique credential identifier
    pub credential_id: Uuid,

    /// Issuance timestamp
    pub issued_at: DateTime<Utc>,

    /// Expiration timestamp
    pub expires_at: DateTime<Utc>,

    /// Human principal (visible to gateway, not tool provider)
    pub principal: Principal,

    /// Agent identity (visible to gateway, not tool provider)
    pub agent: Agent,

    /// ARC credential (presented to tool provider)
    pub arc_credential: ARCCredential,

    /// MCP tool access control
    pub mcp_capabilities: MCPCapabilities,

    /// Delegation rights
    pub delegation: DelegationRights,

    /// Delegation chain (audit trail for gateway)
    pub delegation_chain: DelegationChain,

    /// Gateway signature (verifies principal/agent binding)
    #[serde(with = "signature_serde")]
    pub signature: Signature,

    /// Optional extensions
    #[serde(default)]
    pub extensions: Extensions,
}

impl HybridCredential {
    /// Verify gateway signature (for audit purposes)
    pub fn verify_signature(&self, issuer_public_key: &[u8]) -> Result<()> {
        let public_key = ed25519_compact::PublicKey::from_slice(issuer_public_key)
            .map_err(|e| ACDPError::CryptoError(format!("Invalid public key: {}", e)))?;

        let signing_data = self.signing_data()?;

        public_key
            .verify(&signing_data, &self.signature)
            .map_err(|e| {
                ACDPError::InvalidCredential(format!("Signature verification failed: {}", e))
            })?;

        Ok(())
    }

    /// Get canonical signing data
    fn signing_data(&self) -> Result<Vec<u8>> {
        let mut data = Vec::new();

        data.extend_from_slice(self.version.as_bytes());
        data.extend_from_slice(self.credential_id.as_bytes());
        data.extend_from_slice(&self.issued_at.timestamp().to_le_bytes());
        data.extend_from_slice(&self.expires_at.timestamp().to_le_bytes());

        // Use serde_json for cross-platform compatibility
        let canonical = serde_json::to_vec(&(
            &self.principal,
            &self.agent,
            &self.mcp_capabilities,
            &self.delegation,
        ))
        .map_err(|e| ACDPError::InternalError(format!("Serialization failed: {}", e)))?;

        data.extend_from_slice(&canonical);

        Ok(data)
    }

    /// Create anonymous presentation (for tool provider)
    pub fn create_anonymous_presentation(
        &self,
        presentation_context: &[u8],
        nonce: u64,
    ) -> Result<ARCPresentation> {
        use crate::arc::ARCGenerators;
        let generators = ARCGenerators::new();
        self.arc_credential
            .create_presentation(presentation_context, nonce, &generators)
    }

    /// Check rate limit
    pub fn check_rate_limit(&self) -> Result<u64> {
        Ok(self.arc_credential.presentations_remaining())
    }
}

/// Credential extensions (future use)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Extensions {
    /// AP2 payment mandate link (strategic partnership)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ap2_mandate_link: Option<String>,

    /// Custom metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credential_type_serialization() {
        let json = serde_json::to_string(&CredentialType::IdentityBound).unwrap();
        assert_eq!(json, "\"identity_bound\"");

        let deserialized: CredentialType = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, CredentialType::IdentityBound);
    }
}
