//! Delegation endpoint
//!
//! POST /acdp/v1/credentials/delegate
//! Creates delegated credentials (Agent A → Agent B).

use crate::AppState;
use acdp_common::{types::*, ACDPGatewayError, Result};
use actix_web::{post, web, HttpResponse, Responder};

#[post("/credentials/delegate")]
pub async fn delegate_credential(
    _state: web::Data<AppState>,
    _body: web::Json<DelegationRequest>,
) -> Result<impl Responder> {
    // TODO: Implement delegation
    // 1. Verify parent credential exists and allows delegation
    // 2. Verify child capabilities are subset of parent
    // 3. Create new credential with delegation chain
    // 4. Store in database

    Err(ACDPGatewayError::InternalError(
        "Delegation not yet implemented".to_string(),
    ))
}
