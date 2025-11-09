//! ACDP Gateway Binary
//!
//! HTTP server for credential issuance, delegation, and verification.

use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use anyhow::Result;
use acdp_auth::{
    gateway::{ACDPGateway, CredentialIssuanceRequest, CredentialIssuanceResponse},
    verification::{VerificationRequest, VerificationResult},
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

/// Main entry point
#[actix_web::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    // Create ACDP Gateway
    let gateway = Arc::new(Mutex::new(ACDPGateway::new(
        "https://acdp-gateway.kontext.dev/".to_string(),
    )));

    info!("Starting ACDP Gateway on http://0.0.0.0:8080");

    // Start HTTP server
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(gateway.clone()))
            .route("/health", web::get().to(health_check))
            .route(
                "/acdp/v1/credentials/issue",
                web::post().to(issue_credential),
            )
            .route("/acdp/v1/verify", web::post().to(verify_credential))
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await?;

    Ok(())
}

/// Health check endpoint
async fn health_check() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// Issue credential endpoint
async fn issue_credential(
    _gateway: web::Data<Arc<Mutex<ACDPGateway>>>,
    _request: web::Json<CredentialIssuanceRequest>,
) -> impl Responder {
    // TODO: Implement credential issuance
    // 1. Extract and verify ID-JAG from Authorization header
    // 2. Call gateway.issue_credential()
    // 3. Return CredentialIssuanceResponse

    warn!("Credential issuance not yet implemented");

    HttpResponse::NotImplemented().json(serde_json::json!({
        "error": "not_implemented",
        "message": "Credential issuance not yet implemented"
    }))
}

/// Verify credential endpoint
async fn verify_credential(
    _gateway: web::Data<Arc<Mutex<ACDPGateway>>>,
    _request: web::Json<VerificationRequest>,
) -> impl Responder {
    // TODO: Implement credential verification
    // 1. Parse credential from request
    // 2. Call gateway.verify_credential()
    // 3. Return VerificationResult

    warn!("Credential verification not yet implemented");

    HttpResponse::NotImplemented().json(serde_json::json!({
        "error": "not_implemented",
        "message": "Credential verification not yet implemented"
    }))
}
