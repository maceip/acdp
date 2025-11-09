//! ACDP Credential Service
//!
//! Core business logic for issuing and verifying ACDP credentials.

use acdp_common::{
    types::*,
    ACDPGatewayError, Result,
};
use chrono::{DateTime, Duration, Utc};
use mcp_auth::{
    agent::Agent,
    arc::{ARCCredentialRequest, ARCCredentialResponse, ARCGenerators, ClientSecrets, ServerPrivateKey, ServerPublicKey},
    capabilities::MCPCapabilities,
    credentials::{ACDPCredential, AnonymousCredential, HybridCredential, IdentityBoundCredential},
    delegation::{DelegationChain, DelegationRights},
    principal::Principal,
};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct CredentialService {
    db_pool: PgPool,
    signing_key: Vec<u8>,
    public_key: Vec<u8>,
    gateway_issuer: String,
    arc_generators: Arc<ARCGenerators>,
    arc_server_key: Arc<ServerPrivateKey>,
    arc_server_pubkey: Arc<ServerPublicKey>,
}

impl CredentialService {
    pub fn new(
        db_pool: PgPool,
        signing_key: Vec<u8>,
        public_key: Vec<u8>,
        gateway_issuer: String,
    ) -> Self {
        let arc_generators = Arc::new(ARCGenerators::new());
        let arc_server_key = Arc::new(ServerPrivateKey::random());
        let arc_server_pubkey = Arc::new(ServerPublicKey::from_private_key(
            &arc_server_key,
            &arc_generators,
        ));

        Self {
            db_pool,
            signing_key,
            public_key,
            gateway_issuer,
            arc_generators,
            arc_server_key,
            arc_server_pubkey,
        }
    }

    /// Issue ACDP credential
    pub async fn issue_credential(
        &self,
        principal: Principal,
        request: CredentialIssuanceRequest,
    ) -> Result<ACDPCredential> {
        // Parse agent public key
        let agent_pubkey = hex::decode(&request.agent_public_key)
            .map_err(|e| ACDPGatewayError::CredentialIssuanceFailed(format!(
                "Invalid agent public key: {}",
                e
            )))?;

        // Create agent
        let agent = Agent {
            agent_id: request.agent_id.clone(),
            public_key: agent_pubkey,
            agent_type: "mcp-client".to_string(),
            metadata: None,
        };

        // Parse capabilities
        let mcp_capabilities = self.parse_mcp_capabilities(&request.capabilities)?;

        // Calculate expiration
        let issued_at = Utc::now();
        let expires_at = issued_at + Duration::days(request.duration_days as i64);

        let credential_id = Uuid::new_v4();

        // Issue credential based on type
        let credential = match request.credential_type {
            CredentialType::IdentityBound => {
                self.issue_identity_bound(
                    credential_id,
                    principal,
                    agent,
                    mcp_capabilities,
                    issued_at,
                    expires_at,
                    &request.capabilities.rate_limit,
                )?
            }
            CredentialType::Anonymous => {
                self.issue_anonymous(
                    credential_id,
                    mcp_capabilities,
                    issued_at,
                    expires_at,
                    &request.capabilities.rate_limit,
                ).await?
            }
            CredentialType::Hybrid => {
                self.issue_hybrid(
                    credential_id,
                    principal,
                    agent,
                    mcp_capabilities,
                    issued_at,
                    expires_at,
                    &request.capabilities.rate_limit,
                ).await?
            }
        };

        // Store in database
        self.store_credential(&credential, &request).await?;

        Ok(credential)
    }

    fn issue_identity_bound(
        &self,
        credential_id: Uuid,
        principal: Principal,
        agent: Agent,
        mcp_capabilities: MCPCapabilities,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        _rate_limit: &RateLimitConfig,
    ) -> Result<ACDPCredential> {
        use ed25519_compact::{KeyPair, Signature};

        // Create delegation rights (no delegation for base credential)
        let delegation = DelegationRights {
            can_delegate: false,
            max_depth: 0,
            allowed_capabilities: vec![],
        };

        // Create delegation chain (empty for base credential)
        let delegation_chain = DelegationChain::new();

        // Create signing data
        let mut signing_data = Vec::new();
        signing_data.extend_from_slice(b"ACDP-v0.3");
        signing_data.extend_from_slice(credential_id.as_bytes());
        signing_data.extend_from_slice(&issued_at.timestamp().to_le_bytes());
        signing_data.extend_from_slice(&expires_at.timestamp().to_le_bytes());

        // Sign with gateway key
        let keypair = KeyPair::from_seed(ed25519_compact::Seed::from_slice(&self.signing_key).unwrap());
        let signature = keypair.sk.sign(&signing_data, None);

        Ok(ACDPCredential::IdentityBound(IdentityBoundCredential {
            version: "0.3".to_string(),
            credential_id,
            issued_at,
            expires_at,
            principal,
            agent,
            mcp_capabilities,
            delegation,
            delegation_chain,
            signature,
            extensions: Default::default(),
        }))
    }

    async fn issue_anonymous(
        &self,
        credential_id: Uuid,
        mcp_capabilities: MCPCapabilities,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        rate_limit: &RateLimitConfig,
    ) -> Result<ACDPCredential> {
        // Generate client secrets
        let client_secrets = ClientSecrets::random();

        // Create ARC credential request
        let arc_request = ARCCredentialRequest::new(
            &client_secrets,
            &self.arc_server_pubkey,
            &self.arc_generators,
        );

        // Server issues ARC credential
        let m2 = p256::Scalar::from(0u64); // Attribute m2 (e.g., bucket ID)
        let arc_response = ARCCredentialResponse::issue(
            &arc_request,
            &self.arc_server_key,
            m2,
            &self.arc_generators,
        ).map_err(|e| ACDPGatewayError::CredentialIssuanceFailed(format!(
            "ARC issuance failed: {}",
            e
        )))?;

        // Client finalizes ARC credential
        let arc_credential = mcp_auth::arc::ARCCredential::from_response(
            &arc_response,
            &arc_request,
            &client_secrets,
            &self.arc_server_pubkey,
        ).map_err(|e| ACDPGatewayError::CredentialIssuanceFailed(format!(
            "ARC finalization failed: {}",
            e
        )))?;

        Ok(ACDPCredential::Anonymous(AnonymousCredential {
            version: "0.3".to_string(),
            credential_id,
            issued_at,
            expires_at,
            arc_credential,
            mcp_capabilities,
            extensions: Default::default(),
        }))
    }

    async fn issue_hybrid(
        &self,
        credential_id: Uuid,
        principal: Principal,
        agent: Agent,
        mcp_capabilities: MCPCapabilities,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        rate_limit: &RateLimitConfig,
    ) -> Result<ACDPCredential> {
        use ed25519_compact::{KeyPair, Signature};

        // Generate ARC credential (same as anonymous)
        let client_secrets = ClientSecrets::random();
        let arc_request = ARCCredentialRequest::new(
            &client_secrets,
            &self.arc_server_pubkey,
            &self.arc_generators,
        );

        let m2 = p256::Scalar::from(0u64);
        let arc_response = ARCCredentialResponse::issue(
            &arc_request,
            &self.arc_server_key,
            m2,
            &self.arc_generators,
        ).map_err(|e| ACDPGatewayError::CredentialIssuanceFailed(format!(
            "ARC issuance failed: {}",
            e
        )))?;

        let arc_credential = mcp_auth::arc::ARCCredential::from_response(
            &arc_response,
            &arc_request,
            &client_secrets,
            &self.arc_server_pubkey,
        ).map_err(|e| ACDPGatewayError::CredentialIssuanceFailed(format!(
            "ARC finalization failed: {}",
            e
        )))?;

        // Create delegation components
        let delegation = DelegationRights {
            can_delegate: false,
            max_depth: 0,
            allowed_capabilities: vec![],
        };
        let delegation_chain = DelegationChain::new();

        // Sign with gateway key
        let mut signing_data = Vec::new();
        signing_data.extend_from_slice(b"ACDP-v0.3");
        signing_data.extend_from_slice(credential_id.as_bytes());

        let keypair = KeyPair::from_seed(ed25519_compact::Seed::from_slice(&self.signing_key).unwrap());
        let signature = keypair.sk.sign(&signing_data, None);

        Ok(ACDPCredential::Hybrid(HybridCredential {
            version: "0.3".to_string(),
            credential_id,
            issued_at,
            expires_at,
            principal,
            agent,
            arc_credential,
            mcp_capabilities,
            delegation,
            delegation_chain,
            signature,
            extensions: Default::default(),
        }))
    }

    fn parse_mcp_capabilities(
        &self,
        caps: &CredentialCapabilities,
    ) -> Result<MCPCapabilities> {
        Ok(MCPCapabilities {
            allowed_tools: caps.mcp_tools.allowed.clone(),
            denied_tools: caps.mcp_tools.denied.clone(),
            max_calls_per_tool: None,
            rate_limit_window: Some(caps.rate_limit.window.clone()),
        })
    }

    async fn store_credential(
        &self,
        credential: &ACDPCredential,
        request: &CredentialIssuanceRequest,
    ) -> Result<()> {
        let credential_json = serde_json::to_string(credential)?;

        let (principal_subject, principal_issuer) = match credential {
            ACDPCredential::IdentityBound(c) => (Some(c.principal.subject.clone()), Some(c.principal.issuer.clone())),
            ACDPCredential::Hybrid(c) => (Some(c.principal.subject.clone()), Some(c.principal.issuer.clone())),
            ACDPCredential::Anonymous(_) => (None, None),
        };

        let (issued_at, expires_at) = match credential {
            ACDPCredential::IdentityBound(c) => (c.issued_at, c.expires_at),
            ACDPCredential::Anonymous(c) => (c.issued_at, c.expires_at),
            ACDPCredential::Hybrid(c) => (c.issued_at, c.expires_at),
        };

        sqlx::query!(
            r#"
            INSERT INTO acdp_credentials (
                credential_id, credential_type, principal_subject, principal_issuer,
                agent_id, credential_data, max_presentations, presentations_used,
                issued_at, expires_at, revoked
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
            credential.credential_id(),
            credential.credential_type() as i32,
            principal_subject,
            principal_issuer,
            request.agent_id,
            credential_json,
            request.capabilities.rate_limit.max_presentations as i64,
            0i64,
            issued_at,
            expires_at,
            false,
        )
        .execute(&self.db_pool)
        .await?;

        Ok(())
    }
}
