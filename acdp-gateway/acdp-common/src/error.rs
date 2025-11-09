//! Error types for ACDP Gateway

use thiserror::Error;

pub type Result<T> = std::result::Result<T, ACDPGatewayError>;

#[derive(Debug, Error)]
pub enum ACDPGatewayError {
    #[error("Invalid ID-JAG token: {0}")]
    InvalidIDJAG(String),

    #[error("Credential issuance failed: {0}")]
    CredentialIssuanceFailed(String),

    #[error("Credential verification failed: {0}")]
    CredentialVerificationFailed(String),

    #[error("Delegation not allowed: {0}")]
    DelegationNotAllowed(String),

    #[error("Rate limit exceeded: {used}/{max}")]
    RateLimitExceeded { used: u64, max: u64 },

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),

    #[error("Rauthy integration error: {0}")]
    RauthyError(String),

    #[error("MCP Auth error: {0}")]
    MCPAuthError(#[from] mcp_auth::error::ACDPError),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("Internal error: {0}")]
    InternalError(String),
}

impl actix_web::error::ResponseError for ACDPGatewayError {
    fn error_response(&self) -> actix_web::HttpResponse {
        use actix_web::http::StatusCode;
        use actix_web::HttpResponse;

        let status = match self {
            Self::InvalidIDJAG(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::RateLimitExceeded { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::DelegationNotAllowed(_) => StatusCode::FORBIDDEN,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };

        HttpResponse::build(status).json(serde_json::json!({
            "error": self.to_string(),
        }))
    }
}
