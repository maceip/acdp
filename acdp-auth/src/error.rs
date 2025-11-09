//! Error types for ACDP

/// Result type for ACDP operations
pub type Result<T> = std::result::Result<T, ACDPError>;

/// ACDP-specific errors
#[derive(Debug, thiserror::Error)]
pub enum ACDPError {
    /// Invalid credential format or signature
    #[error("Invalid credential: {0}")]
    InvalidCredential(String),

    /// Credential has expired
    #[error("Credential expired at {0}")]
    CredentialExpired(String),

    /// Rate limit exceeded
    #[error("Rate limit exceeded: {used}/{max} presentations used")]
    RateLimitExceeded {
        /// Presentations used
        used: u64,
        /// Maximum presentations allowed
        max: u64,
    },

    /// Tool not allowed by capabilities
    #[error("Tool '{tool}' not allowed by credential capabilities")]
    ToolNotAllowed {
        /// Tool that was attempted
        tool: String,
    },

    /// Resource limit exceeded
    #[error("Resource limit exceeded: {resource}")]
    ResourceLimitExceeded {
        /// Resource that exceeded limit
        resource: String,
    },

    /// Delegation not allowed
    #[error("Delegation not allowed: {0}")]
    DelegationNotAllowed(String),

    /// Delegation depth exceeded
    #[error("Delegation depth exceeded: {current} > {max}")]
    DelegationDepthExceeded {
        /// Current delegation depth
        current: u8,
        /// Maximum allowed depth
        max: u8,
    },

    /// Capability reduction violation
    #[error("Delegated capabilities must be subset of parent: {0}")]
    CapabilityReductionViolation(String),

    /// Invalid ID-JAG token
    #[error("Invalid ID-JAG token: {0}")]
    InvalidIDJAG(String),

    /// Token exchange failed
    #[error("Token exchange failed: {0}")]
    TokenExchangeFailed(String),

    /// MCP integration error
    #[error("MCP error: {0}")]
    MCPError(String),

    /// ARC proof verification failed
    #[error("ARC proof verification failed: {0}")]
    ARCVerificationFailed(String),

    /// Cryptographic operation failed
    #[error("Cryptographic error: {0}")]
    CryptoError(String),

    /// Database operation failed
    #[error("Database error: {0}")]
    DatabaseError(String),

    /// HTTP request failed
    #[error("HTTP error: {0}")]
    HttpError(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Internal server error
    #[error("Internal error: {0}")]
    InternalError(String),
}

impl ACDPError {
    /// Check if error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ACDPError::HttpError(_) | ACDPError::DatabaseError(_) | ACDPError::InternalError(_)
        )
    }

    /// Get HTTP status code for this error
    pub fn status_code(&self) -> u16 {
        match self {
            ACDPError::InvalidCredential(_) => 401,
            ACDPError::CredentialExpired(_) => 401,
            ACDPError::RateLimitExceeded { .. } => 429,
            ACDPError::ToolNotAllowed { .. } => 403,
            ACDPError::ResourceLimitExceeded { .. } => 403,
            ACDPError::DelegationNotAllowed(_) => 403,
            ACDPError::DelegationDepthExceeded { .. } => 403,
            ACDPError::CapabilityReductionViolation(_) => 400,
            ACDPError::InvalidIDJAG(_) => 401,
            ACDPError::TokenExchangeFailed(_) => 400,
            ACDPError::MCPError(_) => 400,
            ACDPError::ARCVerificationFailed(_) => 401,
            ACDPError::CryptoError(_) => 500,
            ACDPError::DatabaseError(_) => 500,
            ACDPError::HttpError(_) => 502,
            ACDPError::ConfigError(_) => 500,
            ACDPError::InternalError(_) => 500,
        }
    }
}

// Conversions from common error types
impl From<anyhow::Error> for ACDPError {
    fn from(err: anyhow::Error) -> Self {
        ACDPError::InternalError(err.to_string())
    }
}

impl From<serde_json::Error> for ACDPError {
    fn from(err: serde_json::Error) -> Self {
        ACDPError::InternalError(format!("JSON error: {}", err))
    }
}

impl From<reqwest::Error> for ACDPError {
    fn from(err: reqwest::Error) -> Self {
        ACDPError::HttpError(err.to_string())
    }
}
