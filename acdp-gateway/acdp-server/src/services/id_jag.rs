//! ID-JAG (Identity Assertion JWT Authorization Grant) validation
//!
//! Validates ID-JAG tokens issued by enterprise IdPs during token exchange.

use acdp_common::{ACDPGatewayError, Result};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

/// ID-JAG token claims
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IDJAGClaims {
    /// Token type (must be "oauth-id-jag+jwt")
    pub typ: String,

    /// JWT ID
    pub jti: String,

    /// Issuer (enterprise IdP)
    pub iss: String,

    /// Subject (human principal)
    pub sub: String,

    /// Audience (ACDP Gateway)
    pub aud: String,

    /// Resource (MCP Server)
    pub resource: String,

    /// Client ID (MCP Client)
    pub client_id: String,

    /// Expiration time
    pub exp: i64,

    /// Issued at time
    pub iat: i64,

    /// Scope (MCP tools)
    pub scope: String,
}

/// Validate ID-JAG token
pub fn validate_id_jag(
    token: &str,
    expected_audience: &str,
    idp_public_key: &DecodingKey,
) -> Result<IDJAGClaims> {
    let mut validation = Validation::default();
    validation.validate_exp = true;
    validation.set_audience(&[expected_audience]);

    let token_data = decode::<IDJAGClaims>(token, idp_public_key, &validation)
        .map_err(|e| ACDPGatewayError::InvalidIDJAG(format!("JWT decode failed: {}", e)))?;

    let claims = token_data.claims;

    // Verify token type
    if claims.typ != "oauth-id-jag+jwt" {
        return Err(ACDPGatewayError::InvalidIDJAG(format!(
            "Invalid token type: {}",
            claims.typ
        )));
    }

    // Verify audience matches
    if claims.aud != expected_audience {
        return Err(ACDPGatewayError::InvalidIDJAG(format!(
            "Audience mismatch: {} != {}",
            claims.aud, expected_audience
        )));
    }

    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};

    #[test]
    fn test_valid_id_jag() {
        let secret = b"test-secret";
        let encoding_key = EncodingKey::from_secret(secret);
        let decoding_key = DecodingKey::from_secret(secret);

        let claims = IDJAGClaims {
            typ: "oauth-id-jag+jwt".to_string(),
            jti: "test-jti".to_string(),
            iss: "https://idp.example.com".to_string(),
            sub: "alice@example.com".to_string(),
            aud: "https://acdp-gateway.kontext.dev/".to_string(),
            resource: "https://mcp-server.example.com/".to_string(),
            client_id: "mcp-client".to_string(),
            exp: chrono::Utc::now().timestamp() + 300,
            iat: chrono::Utc::now().timestamp(),
            scope: "mcp:filesystem:read".to_string(),
        };

        let token = encode(&Header::default(), &claims, &encoding_key).unwrap();

        let result = validate_id_jag(
            &token,
            "https://acdp-gateway.kontext.dev/",
            &decoding_key,
        );

        assert!(result.is_ok());
        let validated_claims = result.unwrap();
        assert_eq!(validated_claims.sub, "alice@example.com");
    }

    #[test]
    fn test_invalid_token_type() {
        let secret = b"test-secret";
        let encoding_key = EncodingKey::from_secret(secret);
        let decoding_key = DecodingKey::from_secret(secret);

        let claims = IDJAGClaims {
            typ: "invalid-type".to_string(),
            jti: "test-jti".to_string(),
            iss: "https://idp.example.com".to_string(),
            sub: "alice@example.com".to_string(),
            aud: "https://acdp-gateway.kontext.dev/".to_string(),
            resource: "https://mcp-server.example.com/".to_string(),
            client_id: "mcp-client".to_string(),
            exp: chrono::Utc::now().timestamp() + 300,
            iat: chrono::Utc::now().timestamp(),
            scope: "mcp:filesystem:read".to_string(),
        };

        let token = encode(&Header::default(), &claims, &encoding_key).unwrap();

        let result = validate_id_jag(
            &token,
            "https://acdp-gateway.kontext.dev/",
            &decoding_key,
        );

        assert!(result.is_err());
    }
}
