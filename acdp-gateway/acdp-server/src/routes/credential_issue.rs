//! Credential issuance endpoint
//!
//! POST /acdp/v1/credentials/issue
//! Issues ACDP credentials based on ID-JAG tokens.

use crate::{services::id_jag, AppState};
use acdp_common::{types::CredentialIssuanceRequest, ACDPGatewayError, Result};
use actix_web::{post, web, HttpRequest, HttpResponse, Responder};
use jsonwebtoken::DecodingKey;
use mcp_auth::principal::Principal;

#[post("/credentials/issue")]
pub async fn issue_credential(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<CredentialIssuanceRequest>,
) -> Result<impl Responder> {
    // Extract ID-JAG from Authorization header
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| ACDPGatewayError::Unauthorized("Missing Authorization header".to_string()))?;

    if !auth_header.starts_with("Bearer ") {
        return Err(ACDPGatewayError::Unauthorized(
            "Invalid Authorization header format".to_string(),
        ));
    }

    let id_jag_token = &auth_header[7..];

    // TODO: Get IdP public key dynamically based on token issuer
    // For now, use a placeholder key
    let idp_public_key = DecodingKey::from_secret(b"test-secret");

    // Validate ID-JAG token
    let id_jag_claims = id_jag::validate_id_jag(
        id_jag_token,
        &state.config.gateway_issuer,
        &idp_public_key,
    )?;

    // Create principal from ID-JAG claims
    let principal = Principal::from_id_jag(
        &id_jag_claims.sub,
        &id_jag_claims.iss,
        &id_jag_claims.client_id,
    )
    .map_err(|e| ACDPGatewayError::CredentialIssuanceFailed(format!(
        "Invalid principal: {}",
        e
    )))?;

    // Validate request
    body.validate()
        .map_err(|e| ACDPGatewayError::CredentialIssuanceFailed(format!(
            "Invalid request: {}",
            e
        )))?;

    // Issue credential
    let credential = state
        .credential_service
        .issue_credential(principal, body.into_inner())
        .await?;

    // Serialize credential
    let credential_json = serde_json::to_value(&credential)?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "credential": credential_json,
        "credential_id": credential.credential_id(),
        "credential_type": credential.credential_type(),
    })))
}
