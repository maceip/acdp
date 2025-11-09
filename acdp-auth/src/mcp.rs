//! MCP Enterprise-Managed Authorization
//!
//! Implements ID-JAG (Identity Assertion JWT Authorization Grant) flow.

use crate::error::{ACDPError, Result};
use chrono::Utc;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use validator::Validate;

/// ID-JAG Token
///
/// Identity Assertion JWT Authorization Grant
/// Used for token exchange: ID Token → ID-JAG → ACDP Credential
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct IDJAGToken {
    /// Token type (must be "oauth-id-jag+jwt")
    #[serde(rename = "typ")]
    pub token_type: String,

    /// JWT ID (unique identifier)
    pub jti: String,

    /// Issuer (enterprise IdP)
    #[validate(url)]
    pub iss: String,

    /// Subject (human principal)
    #[validate(length(min = 1))]
    pub sub: String,

    /// Audience (ACDP Gateway)
    #[validate(url)]
    pub aud: String,

    /// Resource (MCP Server)
    #[validate(url)]
    pub resource: String,

    /// Client ID (MCP Client)
    #[validate(length(min = 1))]
    pub client_id: String,

    /// Expiration time
    pub exp: i64,

    /// Issued at time
    pub iat: i64,

    /// Scope (MCP tools)
    pub scope: String,

    /// Optional additional claims
    #[serde(flatten)]
    pub additional_claims: Option<serde_json::Map<String, serde_json::Value>>,
}

impl IDJAGToken {
    /// Create a new ID-JAG token
    pub fn new(
        jti: String,
        iss: String,
        sub: String,
        aud: String,
        resource: String,
        client_id: String,
        scope: String,
        ttl_seconds: i64,
    ) -> Result<Self> {
        let now = Utc::now().timestamp();

        let token = Self {
            token_type: "oauth-id-jag+jwt".to_string(),
            jti,
            iss,
            sub,
            aud,
            resource,
            client_id,
            exp: now + ttl_seconds,
            iat: now,
            scope,
            additional_claims: None,
        };

        token
            .validate()
            .map_err(|e| ACDPError::InvalidIDJAG(format!("Invalid ID-JAG token: {}", e)))?;

        Ok(token)
    }

    /// Check if token is expired
    pub fn is_expired(&self) -> bool {
        Utc::now().timestamp() > self.exp
    }

    /// Verify token claims
    pub fn verify(&self, expected_aud: &str, expected_resource: Option<&str>) -> Result<()> {
        // Check expiration
        if self.is_expired() {
            return Err(ACDPError::TokenExchangeFailed(
                "ID-JAG token expired".to_string(),
            ));
        }

        // Check token type
        if self.token_type != "oauth-id-jag+jwt" {
            return Err(ACDPError::InvalidIDJAG(format!(
                "Invalid token type: {}",
                self.token_type
            )));
        }

        // Check audience
        if self.aud != expected_aud {
            return Err(ACDPError::InvalidIDJAG(format!(
                "Audience mismatch: {} != {}",
                self.aud, expected_aud
            )));
        }

        // Check resource (if provided)
        if let Some(expected_res) = expected_resource {
            if self.resource != expected_res {
                return Err(ACDPError::InvalidIDJAG(format!(
                    "Resource mismatch: {} != {}",
                    self.resource, expected_res
                )));
            }
        }

        Ok(())
    }

    /// Encode to JWT
    pub fn encode(&self, signing_key: &EncodingKey) -> Result<String> {
        encode(&Header::default(), self, signing_key)
            .map_err(|e| ACDPError::TokenExchangeFailed(format!("Failed to encode JWT: {}", e)))
    }

    /// Decode from JWT
    pub fn decode(token: &str, verification_key: &DecodingKey) -> Result<Self> {
        let mut validation = Validation::default();
        validation.validate_exp = false; // We'll validate manually
        validation.validate_aud = false; // Audience validated in verify()

        let token_data = decode::<IDJAGToken>(token, verification_key, &validation)
            .map_err(|e| ACDPError::InvalidIDJAG(format!("Failed to decode JWT: {}", e)))?;

        Ok(token_data.claims)
    }
}

/// Token Exchange Request
///
/// OAuth 2.0 Token Exchange request (RFC 8693)
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct TokenExchangeRequest {
    /// Grant type (must be "urn:ietf:params:oauth:grant-type:token-exchange")
    pub grant_type: String,

    /// Requested token type (must be "urn:ietf:params:oauth:token-type:id-jag")
    pub requested_token_type: String,

    /// Audience (ACDP Gateway)
    #[validate(url)]
    pub audience: String,

    /// Resource (MCP Server)
    #[validate(url)]
    pub resource: String,

    /// Scope (MCP tools)
    pub scope: String,

    /// Subject token (ID Token from IdP)
    #[validate(length(min = 1))]
    pub subject_token: String,

    /// Subject token type (must be "urn:ietf:params:oauth:token-type:id_token")
    pub subject_token_type: String,

    /// Client ID (MCP Client)
    #[validate(length(min = 1))]
    pub client_id: String,

    /// Client secret (for client authentication)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
}

impl TokenExchangeRequest {
    /// Create a new token exchange request
    pub fn new(
        audience: String,
        resource: String,
        scope: String,
        subject_token: String,
        client_id: String,
        client_secret: Option<String>,
    ) -> Result<Self> {
        let request = Self {
            grant_type: "urn:ietf:params:oauth:grant-type:token-exchange".to_string(),
            requested_token_type: "urn:ietf:params:oauth:token-type:id-jag".to_string(),
            audience,
            resource,
            scope,
            subject_token,
            subject_token_type: "urn:ietf:params:oauth:token-type:id_token".to_string(),
            client_id,
            client_secret,
        };

        request.validate().map_err(|e| {
            ACDPError::TokenExchangeFailed(format!("Invalid token exchange request: {}", e))
        })?;

        Ok(request)
    }

    /// Validate token exchange request
    pub fn validate_request(&self) -> Result<()> {
        // Check grant type
        if self.grant_type != "urn:ietf:params:oauth:grant-type:token-exchange" {
            return Err(ACDPError::TokenExchangeFailed(format!(
                "Invalid grant type: {}",
                self.grant_type
            )));
        }

        // Check requested token type
        if self.requested_token_type != "urn:ietf:params:oauth:token-type:id-jag" {
            return Err(ACDPError::TokenExchangeFailed(format!(
                "Invalid requested token type: {}",
                self.requested_token_type
            )));
        }

        // Check subject token type
        if self.subject_token_type != "urn:ietf:params:oauth:token-type:id_token" {
            return Err(ACDPError::TokenExchangeFailed(format!(
                "Invalid subject token type: {}",
                self.subject_token_type
            )));
        }

        Ok(())
    }
}

/// Token Exchange Response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenExchangeResponse {
    /// Issued token type
    pub issued_token_type: String,

    /// Access token (ID-JAG JWT)
    pub access_token: String,

    /// Token type (must be "N_A" for ID-JAG)
    pub token_type: String,

    /// Scope
    pub scope: String,

    /// Expires in seconds
    pub expires_in: i64,
}

impl TokenExchangeResponse {
    /// Create a new token exchange response
    pub fn new(id_jag_token: String, scope: String, expires_in: i64) -> Self {
        Self {
            issued_token_type: "urn:ietf:params:oauth:token-type:id-jag".to_string(),
            access_token: id_jag_token,
            token_type: "N_A".to_string(),
            scope,
            expires_in,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{DecodingKey, EncodingKey};

    #[test]
    fn test_id_jag_creation() {
        let token = IDJAGToken::new(
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

        assert_eq!(token.token_type, "oauth-id-jag+jwt");
        assert_eq!(token.sub, "alice@acme.com");
        assert!(!token.is_expired());
    }

    #[test]
    fn test_id_jag_verification() {
        let token = IDJAGToken::new(
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

        // Valid verification
        assert!(token
            .verify(
                "https://acdp-gateway.kontext.dev/",
                Some("https://mcp-server.example.com/")
            )
            .is_ok());

        // Invalid audience
        assert!(token.verify("https://evil.example/", None).is_err());
    }

    #[test]
    fn test_token_exchange_request() {
        let request = TokenExchangeRequest::new(
            "https://acdp-gateway.kontext.dev/".to_string(),
            "https://mcp-server.example.com/".to_string(),
            "mcp:filesystem:read".to_string(),
            "eyJhbGciOiJIUzI1NiIsI...".to_string(),
            "mcp-client".to_string(),
            Some("secret".to_string()),
        )
        .unwrap();

        assert!(request.validate_request().is_ok());
        assert_eq!(
            request.grant_type,
            "urn:ietf:params:oauth:grant-type:token-exchange"
        );
    }

    #[test]
    fn test_jwt_encoding_decoding() {
        let token = IDJAGToken::new(
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

        let secret = b"test-secret";
        let encoding_key = EncodingKey::from_secret(secret);
        let decoding_key = DecodingKey::from_secret(secret);

        // Encode
        let jwt = token.encode(&encoding_key).unwrap();
        assert!(!jwt.is_empty());

        // Decode
        let decoded = IDJAGToken::decode(&jwt, &decoding_key).unwrap();
        assert_eq!(decoded.sub, token.sub);
        assert_eq!(decoded.aud, token.aud);
    }
}
