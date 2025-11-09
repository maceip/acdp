//! Credential Verification
//!
//! Verifies ACDP credentials and enforces rate limits and capabilities.

use crate::{
    arc::ARCPresentation, credentials::ACDPCredential, error::Result, principal::Principal,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Verification Request
///
/// MCP Server sends this to ACDP Gateway to verify a credential.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationRequest {
    /// Credential to verify
    pub credential: ACDPCredential,

    /// Presentation context
    pub context: PresentationContext,

    /// Optional ARC presentation (for anonymous credentials)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arc_presentation: Option<ARCPresentation>,
}

/// Presentation Context
///
/// Describes the context in which a credential is being used.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresentationContext {
    /// MCP tool being accessed
    pub tool: String,

    /// Resource being accessed (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,

    /// Timestamp of presentation
    pub timestamp: DateTime<Utc>,

    /// MCP Server identifier
    pub server_id: String,

    /// Optional request metadata
    #[serde(flatten)]
    pub metadata: Option<serde_json::Map<String, serde_json::Value>>,
}

/// Verification Result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Whether credential is valid
    pub valid: bool,

    /// Human principal (if identity-bound)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub principal: Option<Principal>,

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

impl VerificationResult {
    /// Create successful verification result
    pub fn success(
        principal: Option<Principal>,
        agent_id: Option<String>,
        presentations_remaining: u64,
        delegation_chain: Vec<String>,
    ) -> Self {
        Self {
            valid: true,
            principal,
            agent_id,
            presentations_remaining,
            delegation_chain,
            failure_reason: None,
            verified_at: Utc::now(),
        }
    }

    /// Create failed verification result
    pub fn failure(reason: String) -> Self {
        Self {
            valid: false,
            principal: None,
            agent_id: None,
            presentations_remaining: 0,
            delegation_chain: vec![],
            failure_reason: Some(reason),
            verified_at: Utc::now(),
        }
    }
}

/// Credential Verifier
///
/// Verifies ACDP credentials and enforces policies.
pub struct CredentialVerifier {
    /// Gateway public key (for signature verification)
    pub gateway_public_key: Vec<u8>,

    /// Expected audience (ACDP Gateway URL)
    pub expected_audience: String,
}

impl CredentialVerifier {
    /// Create a new credential verifier
    pub fn new(gateway_public_key: Vec<u8>, expected_audience: String) -> Self {
        Self {
            gateway_public_key,
            expected_audience,
        }
    }

    /// Verify a credential presentation
    pub fn verify(&self, request: &VerificationRequest) -> VerificationResult {
        // Verify credential signature
        if let Err(e) = request
            .credential
            .verify_signature(&self.gateway_public_key)
        {
            return VerificationResult::failure(format!("Signature verification failed: {}", e));
        }

        // Check expiration
        if request.credential.is_expired() {
            return VerificationResult::failure("Credential expired".to_string());
        }

        // Check tool access
        let capabilities = request.credential.mcp_capabilities();
        if let Err(e) = capabilities.is_tool_allowed(&request.context.tool) {
            return VerificationResult::failure(format!("Tool not allowed: {}", e));
        }

        // Check and decrement rate limit
        let presentations_remaining = match self.check_rate_limit(&request.credential) {
            Ok(remaining) => remaining,
            Err(e) => return VerificationResult::failure(format!("Rate limit exceeded: {}", e)),
        };

        // Build verification result
        let (principal, agent_id, delegation_chain) = match &request.credential {
            ACDPCredential::IdentityBound(cred) => (
                Some(cred.principal.clone()),
                Some(cred.agent.agent_id.clone()),
                cred.delegation_chain.audit_trail(),
            ),
            ACDPCredential::Anonymous(_) => (None, None, vec![]),
            ACDPCredential::Hybrid(_cred) => (
                None,   // Principal hidden from tool provider
                None,   // Agent ID hidden from tool provider
                vec![], // Delegation chain hidden from tool provider
            ),
        };

        VerificationResult::success(
            principal,
            agent_id,
            presentations_remaining,
            delegation_chain,
        )
    }

    /// Check rate limit and decrement if valid
    fn check_rate_limit(&self, credential: &ACDPCredential) -> Result<u64> {
        match credential {
            ACDPCredential::IdentityBound(_) => {
                // Identity-bound credentials use server-side rate limiting, not ARC
                // Return max value to indicate no client-side limit
                Ok(u64::MAX)
            }
            ACDPCredential::Anonymous(cred) => cred.check_rate_limit(),
            ACDPCredential::Hybrid(cred) => cred.check_rate_limit(),
        }
    }
}

/// Verification Cache Entry
///
/// Caches verification results to reduce load on gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationCacheEntry {
    /// Credential ID
    pub credential_id: uuid::Uuid,

    /// Verification result
    pub result: VerificationResult,

    /// Cache timestamp
    pub cached_at: DateTime<Utc>,

    /// TTL (seconds)
    pub ttl_seconds: u64,
}

impl VerificationCacheEntry {
    /// Check if cache entry is expired
    pub fn is_expired(&self) -> bool {
        let now = Utc::now();
        let expiry = self.cached_at + chrono::Duration::seconds(self.ttl_seconds as i64);

        now > expiry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::principal::Principal;
    use uuid::Uuid;

    #[test]
    fn test_verification_result_success() {
        let principal =
            Principal::from_id_jag("alice@acme.com", "https://acme.idp.example", "mcp-client")
                .unwrap();

        let result = VerificationResult::success(
            Some(principal),
            Some("agent://claude".to_string()),
            999,
            vec!["agent://a → agent://b".to_string()],
        );

        assert!(result.valid);
        assert!(result.principal.is_some());
        assert_eq!(result.presentations_remaining, 999);
        assert_eq!(result.delegation_chain.len(), 1);
    }

    #[test]
    fn test_verification_result_failure() {
        let result = VerificationResult::failure("Credential expired".to_string());

        assert!(!result.valid);
        assert!(result.principal.is_none());
        assert_eq!(result.presentations_remaining, 0);
        assert_eq!(
            result.failure_reason,
            Some("Credential expired".to_string())
        );
    }

    #[test]
    fn test_cache_entry_expiration() {
        let entry = VerificationCacheEntry {
            credential_id: Uuid::new_v4(),
            result: VerificationResult::failure("test".to_string()),
            cached_at: Utc::now() - chrono::Duration::seconds(10),
            ttl_seconds: 5,
        };

        assert!(entry.is_expired());

        let fresh_entry = VerificationCacheEntry {
            credential_id: Uuid::new_v4(),
            result: VerificationResult::failure("test".to_string()),
            cached_at: Utc::now(),
            ttl_seconds: 60,
        };

        assert!(!fresh_entry.is_expired());
    }
}
