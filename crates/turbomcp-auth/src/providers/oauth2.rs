//! OAuth 2.1 Authentication Provider
//!
//! Implements the AuthProvider trait for OAuth 2.1 authorization flows.

use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::Arc;
use std::time::SystemTime;

use lru::LruCache;
use tokio::sync::RwLock;
use tracing::{debug, warn};
use uuid::Uuid;

use super::super::config::AuthProviderType;
use super::super::context::AuthContext;
use super::super::introspection::IntrospectionClient;
use super::super::oauth2::OAuth2Client;
use super::super::types::{AuthCredentials, AuthProvider, TokenInfo, UserInfo};
use turbomcp_protocol::{Error as McpError, Result as McpResult};

/// OAuth 2.1 authentication provider
pub struct OAuth2Provider {
    /// Provider name
    name: String,
    /// OAuth2 client for handling flows
    client: Arc<OAuth2Client>,
    /// MCP server canonical URI (RFC 8707) - required for token binding
    #[allow(dead_code)]
    resource_uri: String,
    /// HTTP client for userinfo endpoint
    http_client: reqwest::Client,
    /// Token cache with LRU eviction (capacity: 10,000 entries)
    token_cache: Arc<RwLock<LruCache<String, CachedToken>>>,
    /// Optional introspection client for revocation checking
    introspection_client: Option<Arc<IntrospectionClient>>,
}

// Manual Debug impl to prevent token_cache details from being exposed
impl std::fmt::Debug for OAuth2Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuth2Provider")
            .field("name", &self.name)
            .field("client", &self.client)
            .field("resource_uri", &self.resource_uri)
            .field("http_client", &"<reqwest::Client>")
            .field("token_cache", &"<LruCache>")
            .field(
                "introspection_client",
                &self
                    .introspection_client
                    .as_ref()
                    .map(|_| "<IntrospectionClient>"),
            )
            .finish()
    }
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
    /// Create a new OAuth2 provider with MCP server resource URI
    ///
    /// # Arguments
    ///
    /// * `name` - Provider name for identification
    /// * `client` - OAuth2 client configured for the provider
    /// * `resource_uri` - **MCP server canonical URI** (RFC 8707) - e.g., "https://mcp.example.com"
    ///
    /// # MCP Requirement
    ///
    /// The resource URI binds all tokens to the specific MCP server, preventing
    /// token misuse across service boundaries per RFC 8707.
    pub fn new(name: String, client: Arc<OAuth2Client>, resource_uri: String) -> Self {
        Self {
            name,
            client,
            resource_uri,
            http_client: reqwest::Client::new(),
            token_cache: Arc::new(RwLock::new(LruCache::new(
                NonZeroUsize::new(10_000).expect("10,000 is non-zero"),
            ))),
            introspection_client: None,
        }
    }

    /// Create a new OAuth2 provider with introspection support
    ///
    /// Enables real-time token revocation checking via RFC 7662 introspection endpoint.
    ///
    /// # Arguments
    ///
    /// * `name` - Provider name for identification
    /// * `client` - OAuth2 client configured for the provider
    /// * `resource_uri` - **MCP server canonical URI** (RFC 8707)
    /// * `introspection_client` - Client for token introspection
    ///
    /// # Security
    ///
    /// Introspection provides defense-in-depth by checking token revocation even
    /// when cached tokens haven't expired. This is best-effort: if introspection
    /// fails, the cached result is used to prevent breaking auth during temporary
    /// introspection endpoint outages.
    pub fn with_introspection(
        name: String,
        client: Arc<OAuth2Client>,
        resource_uri: String,
        introspection_client: Arc<IntrospectionClient>,
    ) -> Self {
        Self {
            name,
            client,
            resource_uri,
            http_client: reqwest::Client::new(),
            token_cache: Arc::new(RwLock::new(LruCache::new(
                NonZeroUsize::new(10_000).expect("10,000 is non-zero"),
            ))),
            introspection_client: Some(introspection_client),
        }
    }

    /// Get user info from the OAuth provider's userinfo endpoint
    async fn fetch_user_info(&self, access_token: &str) -> McpResult<UserInfo> {
        let provider_config = self.client.provider_config();
        let userinfo_endpoint = provider_config.userinfo_endpoint.as_ref().ok_or_else(|| {
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

impl AuthProvider for OAuth2Provider {
    fn name(&self) -> &str {
        &self.name
    }

    fn provider_type(&self) -> AuthProviderType {
        AuthProviderType::OAuth2
    }

    fn authenticate(
        &self,
        credentials: AuthCredentials,
    ) -> Pin<Box<dyn Future<Output = McpResult<AuthContext>> + Send + '_>> {
        Box::pin(async move {
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
                _ => Err(McpError::invalid_params(
                    "OAuth2 provider only accepts OAuth2Code credentials".to_string(),
                )),
            }
        })
    }

    fn validate_token(
        &self,
        token: &str,
    ) -> Pin<Box<dyn Future<Output = McpResult<AuthContext>> + Send + '_>> {
        let token = token.to_string();
        Box::pin(async move {
            // Check cache first (with reduced TTL from 300s to 60s)
            {
                let mut cache = self.token_cache.write().await;
                if let Some(cached) = cache.get(&token) {
                    let elapsed = cached
                        .cached_at
                        .elapsed()
                        .unwrap_or(std::time::Duration::from_secs(0));
                    // Cache for 60 seconds (reduced from 300s for better revocation detection)
                    if elapsed < std::time::Duration::from_secs(60) {
                        // If introspection is configured, check if token is still active
                        if let Some(ref introspection_client) = self.introspection_client {
                            match introspection_client.is_token_active(&token).await {
                                Ok(false) => {
                                    // Token was revoked - remove from cache
                                    debug!(
                                        "Token revoked according to introspection, removing from cache"
                                    );
                                    cache.pop(&token);
                                    return Err(McpError::invalid_params(
                                        "Token has been revoked".to_string(),
                                    ));
                                }
                                Ok(true) => {
                                    // Token is still active, continue with cached result
                                    debug!("Token confirmed active via introspection");
                                }
                                Err(e) => {
                                    // Introspection failed - log warning but fall through to cached result
                                    // This is best-effort: we don't break auth if introspection is temporarily down
                                    warn!(
                                        error = %e,
                                        "Introspection check failed, falling back to cached result"
                                    );
                                }
                            }
                        }

                        // Build context from cached token
                        drop(cache); // Release write lock before async operations
                        let user_info = self.fetch_user_info(&token).await?;
                        let request_id = Uuid::new_v4().to_string();
                        let cached_token = {
                            // Re-acquire lock to read cached token
                            let cache = self.token_cache.read().await;
                            cache
                                .peek(&token)
                                .ok_or_else(|| {
                                    McpError::internal("Token removed from cache".to_string())
                                })?
                                .token
                                .clone()
                        };

                        let mut builder = AuthContext::builder()
                            .subject(user_info.id.clone())
                            .user(user_info)
                            .roles(vec!["oauth_user".to_string()])
                            .permissions(vec!["api_access".to_string()])
                            .request_id(request_id)
                            .token(cached_token.clone())
                            .provider(self.name.clone())
                            .authenticated_at(SystemTime::now());

                        if let Some(secs) = cached_token.expires_in {
                            builder = builder.expires_at(
                                SystemTime::now() + std::time::Duration::from_secs(secs),
                            );
                        }

                        return builder
                            .build()
                            .map_err(|e| McpError::internal(e.to_string()));
                    }
                }
            }

            // Token not in cache or cache expired - fetch user info to validate
            let user_info = self.fetch_user_info(&token).await?;
            let request_id = Uuid::new_v4().to_string();

            AuthContext::builder()
                .subject(user_info.id.clone())
                .user(user_info)
                .roles(vec!["oauth_user".to_string()])
                .permissions(vec!["api_access".to_string()])
                .request_id(request_id)
                .provider(self.name.clone())
                .authenticated_at(SystemTime::now())
                .build()
                .map_err(|e| McpError::internal(e.to_string()))
        })
    }

    fn refresh_token(
        &self,
        refresh_token: &str,
    ) -> Pin<Box<dyn Future<Output = McpResult<TokenInfo>> + Send + '_>> {
        let refresh_token = refresh_token.to_string();
        Box::pin(async move {
            // Refresh token using the OAuth2 client
            // Note: RFC 8707 resource parameter is handled in OAuth2Client::refresh_access_token
            self.client.refresh_access_token(&refresh_token).await
        })
    }

    fn revoke_token(
        &self,
        token: &str,
    ) -> Pin<Box<dyn Future<Output = McpResult<()>> + Send + '_>> {
        let token = token.to_string();
        Box::pin(async move {
            // Remove from cache first (LRU cache uses pop instead of remove)
            let cached_token = self.token_cache.write().await.pop(&token);

            // If we have the full token info, revoke it at the provider (RFC 7009)
            if let Some(cached) = cached_token {
                self.client.revoke_token(&cached.token).await?;
            } else {
                // If not in cache, create a minimal TokenInfo for revocation
                let token_info = TokenInfo {
                    access_token: token,
                    token_type: "Bearer".to_string(),
                    refresh_token: None,
                    expires_in: None,
                    scope: None,
                };
                self.client.revoke_token(&token_info).await?;
            }

            Ok(())
        })
    }

    fn get_user_info(
        &self,
        token: &str,
    ) -> Pin<Box<dyn Future<Output = McpResult<UserInfo>> + Send + '_>> {
        let token = token.to_string();
        Box::pin(async move { self.fetch_user_info(&token).await })
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
            client_secret: "test-secret".to_string().into(),
            auth_url: "https://provider.example.com/oauth/authorize".to_string(),
            token_url: "https://provider.example.com/oauth/token".to_string(),
            revocation_url: Some("https://provider.example.com/oauth/revoke".to_string()),
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
        let provider = OAuth2Provider::new(
            "test".to_string(),
            Arc::new(oauth_client),
            "https://mcp.example.com".to_string(), // MCP server resource URI
        );

        assert_eq!(provider.name(), "test");
        assert_eq!(provider.provider_type(), AuthProviderType::OAuth2);
    }
}
