//! Human Principal
//!
//! Represents the human user who authorized the agent.
//! Extracted from ID-JAG token (Enterprise-Managed Authorization).

use crate::error::{ACDPError, Result};
use serde::{Deserialize, Serialize};
use validator::Validate;

/// Human Principal (from ID-JAG)
///
/// Links agent credentials to a human user via enterprise IdP.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Validate)]
pub struct Principal {
    /// Human identifier (from ID-JAG `sub` claim)
    ///
    /// Examples: "alice@acme.com", "user-123"
    #[validate(length(min = 1, max = 255))]
    pub human_id: String,

    /// Enterprise IdP issuer (from ID-JAG `iss` claim)
    ///
    /// Example: "https://acme.idp.example"
    #[validate(url)]
    pub idp_issuer: String,

    /// MCP client ID (from ID-JAG `client_id` claim)
    ///
    /// Example: "mcp-client"
    #[validate(length(min = 1, max = 255))]
    pub idp_client_id: String,

    /// Optional additional claims from ID-JAG
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additional_claims: Option<AdditionalClaims>,
}

impl Principal {
    /// Create a new principal from ID-JAG claims
    pub fn from_id_jag(
        human_id: impl Into<String>,
        idp_issuer: impl Into<String>,
        idp_client_id: impl Into<String>,
    ) -> Result<Self> {
        let principal = Self {
            human_id: human_id.into(),
            idp_issuer: idp_issuer.into(),
            idp_client_id: idp_client_id.into(),
            additional_claims: None,
        };

        principal
            .validate()
            .map_err(|e| ACDPError::InvalidIDJAG(format!("Invalid principal: {}", e)))?;

        Ok(principal)
    }

    /// Create with additional claims
    pub fn with_claims(mut self, claims: AdditionalClaims) -> Self {
        self.additional_claims = Some(claims);
        self
    }

    /// Get canonical identifier for audit logs
    pub fn canonical_id(&self) -> String {
        format!("{}@{}", self.human_id, self.idp_issuer)
    }

    /// Verify principal matches ID-JAG claims
    pub fn verify_id_jag(&self, sub: &str, iss: &str, client_id: &str) -> Result<()> {
        if self.human_id != sub {
            return Err(ACDPError::InvalidIDJAG(format!(
                "Principal human_id mismatch: {} != {}",
                self.human_id, sub
            )));
        }

        if self.idp_issuer != iss {
            return Err(ACDPError::InvalidIDJAG(format!(
                "Principal idp_issuer mismatch: {} != {}",
                self.idp_issuer, iss
            )));
        }

        if self.idp_client_id != client_id {
            return Err(ACDPError::InvalidIDJAG(format!(
                "Principal idp_client_id mismatch: {} != {}",
                self.idp_client_id, client_id
            )));
        }

        Ok(())
    }
}

/// Additional claims from ID-JAG (optional)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdditionalClaims {
    /// User email (from ID-JAG `email` claim)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// User name (from ID-JAG `name` claim)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Organization (from ID-JAG `org` claim)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub org: Option<String>,

    /// Groups/roles (from ID-JAG `groups` claim)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<String>>,

    /// Custom claims
    #[serde(flatten)]
    pub custom: serde_json::Map<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_principal_creation() {
        let principal =
            Principal::from_id_jag("alice@acme.com", "https://acme.idp.example", "mcp-client")
                .unwrap();

        assert_eq!(principal.human_id, "alice@acme.com");
        assert_eq!(principal.idp_issuer, "https://acme.idp.example");
        assert_eq!(principal.idp_client_id, "mcp-client");
    }

    #[test]
    fn test_canonical_id() {
        let principal =
            Principal::from_id_jag("alice@acme.com", "https://acme.idp.example", "mcp-client")
                .unwrap();

        assert_eq!(
            principal.canonical_id(),
            "alice@acme.com@https://acme.idp.example"
        );
    }

    #[test]
    fn test_verify_id_jag() {
        let principal =
            Principal::from_id_jag("alice@acme.com", "https://acme.idp.example", "mcp-client")
                .unwrap();

        // Valid verification
        assert!(principal
            .verify_id_jag("alice@acme.com", "https://acme.idp.example", "mcp-client")
            .is_ok());

        // Invalid human_id
        assert!(principal
            .verify_id_jag("bob@acme.com", "https://acme.idp.example", "mcp-client")
            .is_err());

        // Invalid issuer
        assert!(principal
            .verify_id_jag("alice@acme.com", "https://evil.example", "mcp-client")
            .is_err());
    }

    #[test]
    fn test_validation() {
        // Empty human_id should fail
        let result = Principal::from_id_jag("", "https://acme.idp.example", "mcp-client");
        assert!(result.is_err());

        // Invalid URL should fail
        let result = Principal::from_id_jag("alice@acme.com", "not-a-url", "mcp-client");
        assert!(result.is_err());
    }
}
