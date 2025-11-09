//! Agent Identity
//!
//! Represents autonomous agents accessing MCP tools.

use crate::error::{ACDPError, Result};
use ed25519_compact::PublicKey;
use serde::{Deserialize, Serialize};
use validator::Validate;

/// Agent Identity
///
/// Represents an autonomous agent (AI assistant, workflow automation, etc.)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Validate)]
pub struct Agent {
    /// Agent identifier
    ///
    /// Format: "agent://{platform}/{instance}"
    /// Examples:
    /// - "agent://claude-assistant"
    /// - "agent://anthropic/claude"
    /// - "agent://openai/gpt-4"
    #[validate(length(min = 1, max = 255))]
    pub agent_id: String,

    /// Agent public key (Ed25519)
    ///
    /// Used for:
    /// - Credential binding (prevents theft)
    /// - Delegation signatures
    #[serde(with = "public_key_serde")]
    pub public_key: PublicKey,

    /// Platform identifier
    ///
    /// Examples: "anthropic/claude", "openai/gpt-4", "custom"
    #[validate(length(min = 1, max = 100))]
    pub platform: String,

    /// Whether gateway verified agent code
    ///
    /// True = Gateway performed code verification (checksum, reproducible build)
    /// False = Unverified agent
    pub verified: bool,

    /// Optional metadata
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<AgentMetadata>,
}

impl Agent {
    /// Create a new agent
    pub fn new(
        agent_id: impl Into<String>,
        public_key: PublicKey,
        platform: impl Into<String>,
        verified: bool,
    ) -> Result<Self> {
        let agent = Self {
            agent_id: agent_id.into(),
            public_key,
            platform: platform.into(),
            verified,
            metadata: None,
        };

        agent
            .validate()
            .map_err(|e| ACDPError::InvalidCredential(format!("Invalid agent: {}", e)))?;

        Ok(agent)
    }

    /// Add metadata
    pub fn with_metadata(mut self, metadata: AgentMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Get public key bytes
    pub fn public_key_bytes(&self) -> &[u8] {
        self.public_key.as_ref()
    }

    /// Verify a signature from this agent
    pub fn verify_signature(&self, message: &[u8], signature: &[u8]) -> Result<()> {
        let sig = ed25519_compact::Signature::from_slice(signature)
            .map_err(|e| ACDPError::CryptoError(format!("Invalid signature: {}", e)))?;

        self.public_key
            .verify(message, &sig)
            .map_err(|e| ACDPError::CryptoError(format!("Signature verification failed: {}", e)))?;

        Ok(())
    }
}

/// Agent Metadata (optional)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMetadata {
    /// Agent version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Code hash (for verification)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_hash: Option<String>,

    /// Build timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_timestamp: Option<chrono::DateTime<chrono::Utc>>,

    /// Capabilities description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities_description: Option<String>,

    /// Custom fields
    #[serde(flatten)]
    pub custom: serde_json::Map<String, serde_json::Value>,
}

/// Serde module for PublicKey serialization
mod public_key_serde {
    use ed25519_compact::PublicKey;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(key: &PublicKey, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let bytes = key.as_ref();
        serializer.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<PublicKey, D::Error>
    where
        D: Deserializer<'de>,
    {
        let hex_string = String::deserialize(deserializer)?;
        let bytes = hex::decode(&hex_string).map_err(serde::de::Error::custom)?;
        PublicKey::from_slice(&bytes).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_compact::KeyPair;

    #[test]
    fn test_agent_creation() {
        let keypair = KeyPair::generate();
        let agent = Agent::new(
            "agent://claude-assistant",
            keypair.pk,
            "anthropic/claude",
            true,
        )
        .unwrap();

        assert_eq!(agent.agent_id, "agent://claude-assistant");
        assert_eq!(agent.platform, "anthropic/claude");
        assert!(agent.verified);
    }

    #[test]
    fn test_signature_verification() {
        let keypair = KeyPair::generate();
        let agent = Agent::new("agent://test", keypair.pk, "test-platform", false).unwrap();

        let message = b"test message";
        let signature = keypair.sk.sign(message, None);

        // Valid signature
        assert!(agent.verify_signature(message, signature.as_ref()).is_ok());

        // Invalid signature
        let bad_signature = [0u8; 64];
        assert!(agent.verify_signature(message, &bad_signature).is_err());
    }

    #[test]
    fn test_serialization() {
        let keypair = KeyPair::generate();
        let agent = Agent::new(
            "agent://claude-assistant",
            keypair.pk,
            "anthropic/claude",
            true,
        )
        .unwrap();

        let json = serde_json::to_string(&agent).unwrap();
        let deserialized: Agent = serde_json::from_str(&json).unwrap();

        assert_eq!(agent, deserialized);
    }
}
