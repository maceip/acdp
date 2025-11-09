//! ACDP Gateway
//!
//! HTTP API for credential issuance, delegation, and verification.

use crate::{
    agent::Agent,
    arc::ARCCredential,
    capabilities::MCPCapabilities,
    credentials::{
        ACDPCredential, AnonymousCredential, CredentialType, HybridCredential,
        IdentityBoundCredential,
    },
    delegation::{DelegationChain, DelegationRights},
    error::{ACDPError, Result},
    mcp::IDJAGToken,
    principal::Principal,
    verification::{VerificationRequest, VerificationResult},
    ACDP_VERSION, DEFAULT_CREDENTIAL_DURATION_DAYS,
};
use chrono::Utc;
use ed25519_compact::{KeyPair, Signature};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Credential Issuance Request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialIssuanceRequest {
    /// Agent identifier
    pub agent_id: String,

    /// Agent public key (hex-encoded)
    pub agent_public_key: String,

    /// Credential type
    pub credential_type: CredentialType,

    /// Capabilities
    pub capabilities: MCPCapabilities,

    /// Credential duration (days)
    #[serde(default = "default_duration")]
    pub duration_days: u64,
}

fn default_duration() -> u64 {
    DEFAULT_CREDENTIAL_DURATION_DAYS
}

/// Credential Issuance Response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialIssuanceResponse {
    /// ACDP credential
    pub credential: ACDPCredential,

    /// Credential ID
    pub credential_id: Uuid,

    /// Expiration timestamp
    pub expires_at: chrono::DateTime<Utc>,

    /// Rate limit state
    pub rate_limit_state: RateLimitState,
}

/// Rate Limit State
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitState {
    /// Maximum presentations
    pub max_presentations: u64,

    /// Presentations remaining
    pub presentations_remaining: u64,
}

/// Delegation Request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationRequest {
    /// Parent credential
    pub parent_credential: ACDPCredential,

    /// Delegate to agent ID
    pub delegate_to_agent: String,

    /// Delegate to public key (hex-encoded)
    pub delegate_to_public_key: String,

    /// Sub-capabilities (must be subset of parent)
    pub sub_capabilities: MCPCapabilities,

    /// Delegation proof
    pub delegation_proof: DelegationProofRequest,
}

/// Delegation Proof Request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationProofRequest {
    /// Parent credential ID
    pub parent_credential_id: Uuid,

    /// Signature from delegating agent
    pub signature: String,
}

/// ACDP Gateway
///
/// Issues, delegates, and verifies ACDP credentials.
pub struct ACDPGateway {
    /// Gateway keypair (for signing credentials)
    keypair: KeyPair,

    /// Gateway issuer URL
    issuer: String,

    /// Database connection (for credential storage)
    #[allow(dead_code)]
    db: std::sync::Arc<tokio::sync::Mutex<()>>, // Placeholder for hiqlite
}

impl ACDPGateway {
    /// Create a new ACDP Gateway
    pub fn new(issuer: String) -> Self {
        let keypair = KeyPair::generate();

        Self {
            keypair,
            issuer,
            db: std::sync::Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// Issue a new credential
    pub async fn issue_credential(
        &self,
        id_jag: &IDJAGToken,
        request: &CredentialIssuanceRequest,
    ) -> Result<CredentialIssuanceResponse> {
        // Validate ID-JAG
        id_jag.verify(&self.issuer, None)?;

        // Create principal from ID-JAG
        let principal = Principal::from_id_jag(
            id_jag.sub.clone(),
            id_jag.iss.clone(),
            id_jag.client_id.clone(),
        )?;

        // Create agent
        let agent_public_key = hex::decode(&request.agent_public_key)
            .map_err(|e| ACDPError::CryptoError(format!("Invalid public key hex: {}", e)))?;

        let public_key = ed25519_compact::PublicKey::from_slice(&agent_public_key)
            .map_err(|e| ACDPError::CryptoError(format!("Invalid public key: {}", e)))?;

        let agent = Agent::new(
            request.agent_id.clone(),
            public_key,
            "custom", // TODO: Extract from agent_id
            false,    // TODO: Implement code verification
        )?;

        // Generate credential based on type
        let credential_id = Uuid::new_v4();
        let now = Utc::now();
        let expires_at = now + chrono::Duration::days(request.duration_days as i64);

        let credential = match request.credential_type {
            CredentialType::IdentityBound => {
                let cred = IdentityBoundCredential {
                    version: ACDP_VERSION.to_string(),
                    credential_id,
                    issued_at: now,
                    expires_at,
                    principal,
                    agent,
                    mcp_capabilities: request.capabilities.clone(),
                    delegation: DelegationRights::allow_delegation(5),
                    delegation_chain: DelegationChain::new(),
                    signature: self.sign_credential(&credential_id)?,
                    extensions: Default::default(),
                };

                ACDPCredential::IdentityBound(cred)
            }

            CredentialType::Anonymous => {
                // Issue ARC credential (simplified - in production would use request/response flow)
                let arc_credential =
                    self.issue_arc_credential(request.capabilities.rate_limit.max_presentations)?;

                let cred = AnonymousCredential {
                    version: ACDP_VERSION.to_string(),
                    credential_id,
                    issued_at: now,
                    expires_at,
                    arc_credential,
                    mcp_capabilities: request.capabilities.clone(),
                    extensions: Default::default(),
                };

                ACDPCredential::Anonymous(cred)
            }

            CredentialType::Hybrid => {
                // Issue ARC credential (simplified - in production would use request/response flow)
                let arc_credential =
                    self.issue_arc_credential(request.capabilities.rate_limit.max_presentations)?;

                let cred = HybridCredential {
                    version: ACDP_VERSION.to_string(),
                    credential_id,
                    issued_at: now,
                    expires_at,
                    principal,
                    agent,
                    arc_credential,
                    mcp_capabilities: request.capabilities.clone(),
                    delegation: DelegationRights::allow_delegation(5),
                    delegation_chain: DelegationChain::new(),
                    signature: self.sign_credential(&credential_id)?,
                    extensions: Default::default(),
                };

                ACDPCredential::Hybrid(cred)
            }
        };

        Ok(CredentialIssuanceResponse {
            credential: credential.clone(),
            credential_id,
            expires_at,
            rate_limit_state: RateLimitState {
                max_presentations: request.capabilities.rate_limit.max_presentations,
                presentations_remaining: request.capabilities.rate_limit.max_presentations,
            },
        })
    }

    /// Verify a credential
    pub async fn verify_credential(&self, request: &VerificationRequest) -> VerificationResult {
        let verifier = crate::verification::CredentialVerifier::new(
            self.keypair.pk.as_ref().to_vec(),
            self.issuer.clone(),
        );

        verifier.verify(request)
    }

    /// Sign credential ID
    fn sign_credential(&self, credential_id: &Uuid) -> Result<Signature> {
        let data = credential_id.as_bytes();
        Ok(self.keypair.sk.sign(data, None))
    }

    /// Get gateway public key
    pub fn public_key(&self) -> Vec<u8> {
        self.keypair.pk.as_ref().to_vec()
    }

    /// Issue ARC credential (simplified internal helper)
    ///
    /// In production, this would be a proper request/response flow with the client.
    /// For now, we simulate the full flow internally.
    fn issue_arc_credential(&self, max_presentations: u64) -> Result<ARCCredential> {
        use crate::arc::{
            ARCCredentialRequest, ARCCredentialResponse, ARCGenerators, ClientSecrets,
            ServerPrivateKey, ServerPublicKey,
        };
        use p256::Scalar;

        // Setup (in production, server would persist these)
        let generators = ARCGenerators::new();
        let server_private_key = ServerPrivateKey::random();
        let server_public_key = ServerPublicKey::from_private_key(&server_private_key, &generators);

        // Client creates request (simulated)
        let client_secrets = ClientSecrets::random();
        let request = ARCCredentialRequest::new(&client_secrets, &server_public_key, &generators);

        // Server issues response
        let m2 = Scalar::from(0u64); // Server attribute (could be bucket ID, etc.)
        let response =
            ARCCredentialResponse::issue(&request, &server_private_key, m2, &generators)?;

        // Client finalizes credential (simulated)
        let mut credential =
            ARCCredential::from_response(&response, &request, &client_secrets, &server_public_key)?;
        credential.max_presentations = max_presentations;

        Ok(credential)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::{RateLimitParams, ResourceLimits, ToolPattern};
    use std::time::Duration;

    #[tokio::test]
    async fn test_credential_issuance() {
        let gateway = ACDPGateway::new("https://acdp-gateway.kontext.dev/".to_string());

        // Create ID-JAG
        let id_jag = IDJAGToken::new(
            "jti-123".to_string(),
            "https://acme.idp.example".to_string(),
            "alice@acme.com".to_string(),
            "https://acdp-gateway.kontext.dev/".to_string(),
            "https://mcp-server.example.com/".to_string(),
            "mcp-client".to_string(),
            "mcp:filesystem:read".to_string(),
            300,
        )
        .unwrap();

        // Create issuance request
        let agent_keypair = KeyPair::generate();
        let request = CredentialIssuanceRequest {
            agent_id: "agent://test".to_string(),
            agent_public_key: hex::encode(agent_keypair.pk.as_ref()),
            credential_type: CredentialType::IdentityBound,
            capabilities: MCPCapabilities {
                allowed_tools: vec![ToolPattern::new("filesystem/*")],
                denied_tools: vec![],
                resource_limits: ResourceLimits::default(),
                rate_limit: RateLimitParams::daily(1000),
            },
            duration_days: 7,
        };

        // Issue credential
        let response = gateway.issue_credential(&id_jag, &request).await.unwrap();

        assert_eq!(response.rate_limit_state.max_presentations, 1000);
        assert_eq!(response.rate_limit_state.presentations_remaining, 1000);

        // Verify it's identity-bound
        match response.credential {
            ACDPCredential::IdentityBound(cred) => {
                assert_eq!(cred.principal.human_id, "alice@acme.com");
                assert_eq!(cred.agent.agent_id, "agent://test");
            }
            _ => panic!("Expected identity-bound credential"),
        }
    }
}
