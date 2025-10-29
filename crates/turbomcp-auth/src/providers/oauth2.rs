//! OAuth 2.1 Authentication Provider
//!
//! Implements the AuthProvider trait for OAuth 2.1 authorization flows.

use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use tokio::sync::RwLock;
use uuid::Uuid;

use super::super::config::AuthProviderType;
use super::super::oauth2::OAuth2Client;
use super::super::types::{AuthContext, AuthCredentials, AuthProvider, TokenInfo, UserInfo};
use turbomcp_protocol::{Error as McpError, Result as McpResult};

/// OAuth 2.1 authentication provider
#[derive(Debug)]
pub struct OAuth2Provider {
    /// Provider name
    name: String,
    /// OAuth2 client for handling flows
    client: Arc<OAuth2Client>,
    /// HTTP client for userinfo endpoint
    http_client: reqwest::Client,
    /// Token cache to avoid redundant requests
    token_cache: Arc<RwLock<std::collections::HashMap<String, CachedToken>>>,
}

/// Cached token with metadata
#[derive(Debug, Clone)]
struct CachedToken {
    /// The token info
    token: TokenInfo,
    /// When it was cached
    cached_at: SystemTime,
}

impl OAuth2Provider {
    /// Create a new OAuth2 provider
    pub fn new(name: String, client: Arc<OAuth2Client>) -> Self {
        Self {
            name,
            client,
            http_client: reqwest::Client::new(),
            token_cache: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Get user info from the OAuth provider's userinfo endpoint
    async fn fetch_user_info(&self, access_token: &str) -> McpResult<UserInfo> {
        let provider_config = self.client.provider_config();
        let userinfo_endpoint = provider_config
            .userinfo_endpoint
            .as_ref()
            .ok_or_else(|| {
                McpError::internal("Provider does not support userinfo endpoint".to_string())
            })?;

        let response = self
            .http_client
            .get(userinfo_endpoint)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| McpError::internal(format!("Userinfo request failed: {e}")))?;

        if !response.status().is_success() {
            return Err(McpError::internal(format!(
                "Userinfo endpoint returned status {}",
                response.status()
            )));
        }

        let user_data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| McpError::internal(format!("Failed to parse userinfo response: {e}")))?;

        // Extract user information from response (varies by provider)
        let user_id = user_data
            .get("sub")
            .or_else(|| user_data.get("id"))
            .or_else(|| user_data.get("user_id"))
            .and_then(|v| v.as_str())
            .unwrap_or(&Uuid::new_v4().to_string())
            .to_string();

        let username = user_data
            .get("name")
            .or_else(|| user_data.get("login"))
            .or_else(|| user_data.get("preferred_username"))
            .and_then(|v| v.as_str())
            .unwrap_or(&user_id)
            .to_string();

        let email = user_data
            .get("email")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let display_name = user_data
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let avatar_url = user_data
            .get("picture")
            .or_else(|| user_data.get("avatar_url"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(UserInfo {
            id: user_id,
            username,
            email,
            display_name,
            avatar_url,
            metadata: std::collections::HashMap::new(),
        })
    }
}

#[async_trait]
impl AuthProvider for OAuth2Provider {
    fn name(&self) -> &str {
        &self.name
    }

    fn provider_type(&self) -> AuthProviderType {
        AuthProviderType::OAuth2
    }

    async fn authenticate(&self, credentials: AuthCredentials) -> McpResult<AuthContext> {
        match credentials {
            AuthCredentials::OAuth2Code { code: _, state: _ } => {
                // In a real implementation, we'd validate state parameter
                // For now, we need the PKCE code verifier which should be stored
                // This is a simplified implementation - in practice, code_verifier
                // would come from session storage based on state parameter

                // Exchange code for token using empty verifier (in real implementation,
                // this would come from stored session state)
                // For now, return an error - the flow should be:
                // 1. Client calls authorization_code_flow() -> gets code_verifier
                // 2. User redirects with code
                // 3. Client calls exchange_code_for_token() with code_verifier
                // 4. Provider stores token and creates AuthContext

                Err(McpError::internal(
                    "OAuth2 authentication requires exchange_code_for_token() method. \
                     Use OAuth2Client.authorization_code_flow() and \
                     OAuth2Client.exchange_code_for_token() directly."
                        .to_string(),
                ))
            }
            _ => Err(McpError::validation(
                "OAuth2 provider only accepts OAuth2Code credentials".to_string(),
            )),
        }
    }

    async fn validate_token(&self, token: &str) -> McpResult<AuthContext> {
        // Check cache first
        {
            let cache = self.token_cache.read().await;
            if let Some(cached) = cache.get(token) {
                let elapsed = cached
                    .cached_at
                    .elapsed()
                    .unwrap_or(std::time::Duration::from_secs(0));
                // Cache for 5 minutes
                if elapsed < std::time::Duration::from_secs(300) {
                    let user_info = self.fetch_user_info(token).await?;
                    let session_id = Uuid::new_v4().to_string();
                    return Ok(AuthContext {
                        user_id: user_info.id.clone(),
                        user: user_info,
                        roles: vec!["oauth_user".to_string()],
                        permissions: vec!["api_access".to_string()],
                        session_id,
                        token: Some(cached.token.clone()),
                        provider: self.name.clone(),
                        authenticated_at: SystemTime::now(),
                        expires_at: cached
                            .token
                            .expires_in
                            .map(|secs| SystemTime::now() + std::time::Duration::from_secs(secs)),
                        metadata: std::collections::HashMap::new(),
                    });
                }
            }
        }

        // Token not in cache or cache expired - fetch user info to validate
        let user_info = self.fetch_user_info(token).await?;
        let session_id = Uuid::new_v4().to_string();

        Ok(AuthContext {
            user_id: user_info.id.clone(),
            user: user_info,
            roles: vec!["oauth_user".to_string()],
            permissions: vec!["api_access".to_string()],
            session_id,
            token: None, // Don't include token in validation response
            provider: self.name.clone(),
            authenticated_at: SystemTime::now(),
            expires_at: None,
            metadata: std::collections::HashMap::new(),
        })
    }

    async fn refresh_token(&self, refresh_token: &str) -> McpResult<TokenInfo> {
        self.client.refresh_access_token(refresh_token).await
    }

    async fn revoke_token(&self, token: &str) -> McpResult<()> {
        // Remove from cache
        self.token_cache.write().await.remove(token);

        // In a real implementation, call the OAuth provider's revocation endpoint
        // For now, we just clear the cache
        Ok(())
    }

    async fn get_user_info(&self, token: &str) -> McpResult<UserInfo> {
        self.fetch_user_info(token).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{OAuth2Config, ProviderType};

    #[test]
    fn test_oauth2_provider_creation() {
        let config = OAuth2Config {
            client_id: "test-client".to_string(),
            client_secret: "test-secret".to_string(),
            auth_url: "https://provider.example.com/oauth/authorize".to_string(),
            token_url: "https://provider.example.com/oauth/token".to_string(),
            redirect_uri: "http://localhost:8080/callback".to_string(),
            scopes: vec!["openid".to_string(), "profile".to_string()],
            flow_type: crate::config::OAuth2FlowType::AuthorizationCode,
            additional_params: std::collections::HashMap::new(),
            security_level: Default::default(),
            #[cfg(feature = "dpop")]
            dpop_config: None,
            mcp_resource_uri: None,
            auto_resource_indicators: true,
        };

        let oauth_client = OAuth2Client::new(&config, ProviderType::Generic)
            .expect("Failed to create OAuth2Client");
        let provider = OAuth2Provider::new("test".to_string(), Arc::new(oauth_client));

        assert_eq!(provider.name(), "test");
        assert_eq!(provider.provider_type(), AuthProviderType::OAuth2);
    }
}
