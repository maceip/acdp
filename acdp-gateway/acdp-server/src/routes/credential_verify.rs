//! Credential verification endpoint
//!
//! POST /acdp/v1/credentials/verify
//! Verifies ACDP credentials for MCP servers.

use crate::AppState;
use acdp_common::{types::*, ACDPGatewayError, Result};
use actix_web::{post, web, HttpResponse, Responder};
use mcp_auth::credentials::ACDPCredential;

#[post("/credentials/verify")]
pub async fn verify_credential(
    state: web::Data<AppState>,
    body: web::Json<CredentialVerificationRequest>,
) -> Result<impl Responder> {
    // Deserialize credential
    let credential: ACDPCredential = serde_json::from_str(&body.credential)
        .map_err(|e| ACDPGatewayError::CredentialVerificationFailed(format!(
            "Invalid credential JSON: {}",
            e
        )))?;

    // Check if credential is expired
    if credential.is_expired() {
        return Ok(HttpResponse::Ok().json(CredentialVerificationResult {
            valid: false,
            principal: None,
            agent_id: None,
            presentations_remaining: 0,
            delegation_chain: vec![],
            failure_reason: Some("Credential expired".to_string()),
            verified_at: chrono::Utc::now(),
        }));
    }

    // Check database for revocation and rate limits
    let stored_cred = sqlx::query!(
        r#"
        SELECT presentations_used, max_presentations, revoked
        FROM acdp_credentials
        WHERE credential_id = $1
        "#,
        credential.credential_id()
    )
    .fetch_optional(&state.db_pool)
    .await?;

    if let Some(cred_data) = stored_cred {
        if cred_data.revoked {
            return Ok(HttpResponse::Ok().json(CredentialVerificationResult {
                valid: false,
                principal: None,
                agent_id: None,
                presentations_remaining: 0,
                delegation_chain: vec![],
                failure_reason: Some("Credential revoked".to_string()),
                verified_at: chrono::Utc::now(),
            }));
        }

        // Check rate limit
        if cred_data.presentations_used >= cred_data.max_presentations {
            return Ok(HttpResponse::Ok().json(CredentialVerificationResult {
                valid: false,
                principal: None,
                agent_id: None,
                presentations_remaining: 0,
                delegation_chain: vec![],
                failure_reason: Some("Rate limit exceeded".to_string()),
                verified_at: chrono::Utc::now(),
            }));
        }

        // Increment presentation counter
        sqlx::query!(
            r#"
            UPDATE acdp_credentials
            SET presentations_used = presentations_used + 1
            WHERE credential_id = $1
            "#,
            credential.credential_id()
        )
        .execute(&state.db_pool)
        .await?;

        // Build successful response
        let (principal, agent_id, delegation_chain) = match &credential {
            ACDPCredential::IdentityBound(c) => (
                Some(PrincipalInfo {
                    subject: c.principal.subject.clone(),
                    issuer: c.principal.issuer.clone(),
                    client_id: c.principal.client_id.clone(),
                }),
                Some(c.agent.agent_id.clone()),
                c.delegation_chain.audit_trail(),
            ),
            ACDPCredential::Anonymous(_) => (None, None, vec![]),
            ACDPCredential::Hybrid(_) => (None, None, vec![]),
        };

        let presentations_remaining = cred_data.max_presentations - (cred_data.presentations_used + 1);

        Ok(HttpResponse::Ok().json(CredentialVerificationResult {
            valid: true,
            principal,
            agent_id,
            presentations_remaining: presentations_remaining as u64,
            delegation_chain,
            failure_reason: None,
            verified_at: chrono::Utc::now(),
        }))
    } else {
        // Credential not found in database
        Ok(HttpResponse::Ok().json(CredentialVerificationResult {
            valid: false,
            principal: None,
            agent_id: None,
            presentations_remaining: 0,
            delegation_chain: vec![],
            failure_reason: Some("Credential not found".to_string()),
            verified_at: chrono::Utc::now(),
        }))
    }
}
