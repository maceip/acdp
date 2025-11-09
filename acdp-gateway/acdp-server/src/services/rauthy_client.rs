//! Rauthy integration client
//!
//! HTTP client for communicating with Rauthy OIDC/OAuth2 server.

use acdp_common::{ACDPGatewayError, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct RauthyClient {
    base_url: String,
    admin_token: String,
    client: reqwest::Client,
}

impl RauthyClient {
    pub fn new(base_url: String, admin_token: String) -> Self {
        Self {
            base_url,
            admin_token,
            client: reqwest::Client::new(),
        }
    }

    /// Verify an ID token with Rauthy
    pub async fn verify_id_token(&self, id_token: &str) -> Result<IDTokenClaims> {
        // In production, this would call Rauthy's token introspection endpoint
        // For now, we'll decode the JWT directly (assuming we have the public key)

        // TODO: Implement proper Rauthy token introspection
        // POST /oidc/introspect with admin token

        Err(ACDPGatewayError::RauthyError(
            "ID token verification not yet implemented".to_string(),
        ))
    }

    /// Get user info from Rauthy
    pub async fn get_user_info(&self, access_token: &str) -> Result<UserInfo> {
        let url = format!("{}/oidc/userinfo", self.base_url);

        let response = self
            .client
            .get(&url)
            .bearer_auth(access_token)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(ACDPGatewayError::RauthyError(format!(
                "User info request failed: {}",
                response.status()
            )));
        }

        let user_info = response.json::<UserInfo>().await?;
        Ok(user_info)
    }

    /// Validate client credentials with Rauthy
    pub async fn validate_client(
        &self,
        client_id: &str,
        client_secret: &str,
    ) -> Result<bool> {
        // TODO: Implement client validation via Rauthy API
        // This would check if the client is authorized for ACDP

        Ok(true) // Placeholder
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IDTokenClaims {
    pub iss: String,
    pub sub: String,
    pub aud: String,
    pub exp: i64,
    pub iat: i64,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub sub: String,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    pub name: Option<String>,
}
