//! OAuth 2.1 Authentication Provider
//!
//! Implements the AuthProvider trait for OAuth 2.1 authorization flows.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::SystemTime;

use moka::future::Cache;
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
    /// MCP server canonical URI (RFC 8707) - required for token binding.
    /// Checked against the token's audience on every `validate_token` call
    /// (see `validate_audience_binding`) — previously stored but never read.
    resource_uri: String,
    /// HTTP client for userinfo endpoint
    http_client: reqwest::Client,
    /// Token cache with LRU eviction (capacity: 10,000 entries, TTL: 300s)
    token_cache: Cache<String, CachedToken>,
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
            .field("token_cache", &"<moka::Cache>")
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
    /// * `resource_uri` - **MCP server canonical URI** (RFC 8707) - e.g., "<https://mcp.example.com>"
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
            token_cache: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(std::time::Duration::from_secs(300))
                .build(),
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
            token_cache: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(std::time::Duration::from_secs(300))
                .build(),
            introspection_client: Some(introspection_client),
        }
    }

    /// Verify the token's audience includes this server's resource URI (RFC
    /// 8707 / MCP: "servers MUST validate that access tokens were issued
    /// specifically for them as the intended audience").
    ///
    /// The audience comes from whichever independent source can name it:
    /// - If introspection is configured, its `aud` field — RFC 7662 responses
    ///   come straight from the authorization server, so this is trustworthy
    ///   without a local signature check.
    /// - Otherwise, an unverified decode of the token's own `aud` claim (if
    ///   it's a compact JWT). This crate has no JWKS for arbitrary third-party
    ///   IdPs here, so the claim can't be cryptographically verified in this
    ///   code path. That's safe *only* because [`Self::validate_token`] treats
    ///   this check as one of two independent gates a token must pass, not
    ///   the sole trust decision: it also requires (via cache or a live
    ///   `fetch_user_info` call) that the provider itself currently accepts
    ///   the token. A token with a forged-but-matching `aud` claim still has
    ///   to be a real, live token the provider recognizes; a merely
    ///   well-formed forgery fails that second gate regardless of what this
    ///   one decided. Callers must keep both checks — don't call this alone
    ///   and treat success as "the token is valid".
    ///
    /// If neither source can name an audience, or the resource URI isn't
    /// configured, this fails closed rather than silently skipping the check.
    async fn validate_audience_binding(&self, token: &str) -> McpResult<()> {
        if self.resource_uri.is_empty() {
            return Err(McpError::internal(
                "OAuth2Provider has no resource_uri configured; refusing to validate tokens \
                 without an audience to check against (RFC 8707)"
                    .to_string(),
            ));
        }

        let audiences = if let Some(ref introspection_client) = self.introspection_client {
            let response = introspection_client
                .introspect(token, Some("access_token"))
                .await?;
            response.aud.as_ref().and_then(aud_claim_to_vec)
        } else {
            jwt_audience_unverified(token)
        };

        let audiences = audiences.ok_or_else(|| {
            McpError::invalid_params(
                "Unable to determine token audience (not a JWT and no introspection endpoint \
                 configured); refusing per RFC 8707"
                    .to_string(),
            )
        })?;

        if audiences
            .iter()
            .any(|aud| crate::server::validate_audience(aud, &self.resource_uri).is_ok())
        {
            Ok(())
        } else {
            Err(McpError::invalid_params(format!(
                "Token audience {audiences:?} does not include this server's resource URI '{}'",
                self.resource_uri
            )))
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
            // RFC 8707 / MCP: validate the token was issued for this server
            // before trusting anything else about it. Checked on every call
            // (cached or not) since the cache only remembers that the token
            // was live at fetch_user_info time, not its audience.
            self.validate_audience_binding(&token).await?;

            // Check moka cache first — thread-safe, no lock required
            if let Some(cached) = self.token_cache.get(&token).await {
                let elapsed = cached
                    .cached_at
                    .elapsed()
                    .unwrap_or(std::time::Duration::from_secs(0));
                // Honor a 60-second inner TTL for revocation detection (shorter than
                // the moka cache TTL of 300s set at construction time)
                if elapsed < std::time::Duration::from_secs(60) {
                    // If introspection is configured, check if token is still active
                    if let Some(ref introspection_client) = self.introspection_client {
                        match introspection_client.is_token_active(&token).await {
                            Ok(false) => {
                                // Token was revoked - remove from cache
                                debug!(
                                    "Token revoked according to introspection, removing from cache"
                                );
                                self.token_cache.invalidate(&token).await;
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
                    let user_info = self.fetch_user_info(&token).await?;
                    let request_id = Uuid::new_v4().to_string();
                    // Re-read from cache — moka returns owned values, so cached above is still valid
                    let cached_token = cached.token.clone();

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
                        builder = builder
                            .expires_at(SystemTime::now() + std::time::Duration::from_secs(secs));
                    }

                    return builder
                        .build()
                        .map_err(|e| McpError::internal(e.to_string()));
                }
            }

            // Token not in cache or inner TTL expired - fetch user info to validate
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
            // Remove from moka cache and retrieve the cached entry if present
            let cached_token = self.token_cache.get(&token).await;
            self.token_cache.invalidate(&token).await;

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
                    issued_at: None,
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

/// Extract audience value(s) from an RFC 7662 introspection response's `aud`
/// field, which per RFC 9068 may be a single string or an array of strings.
fn aud_claim_to_vec(value: &serde_json::Value) -> Option<Vec<String>> {
    match value {
        serde_json::Value::String(s) => Some(vec![s.clone()]),
        serde_json::Value::Array(items) => {
            let strings: Vec<String> = items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            if strings.is_empty() {
                None
            } else {
                Some(strings)
            }
        }
        _ => None,
    }
}

/// Decode a compact JWT's `aud` claim without verifying the signature.
///
/// Returns `None` if `token` isn't a 3-part compact JWT or has no `aud`
/// claim — callers must treat that as "audience unknown", not "audience
/// matches". See [`OAuth2Provider::validate_audience_binding`] for why an
/// unverified decode is acceptable here (it's a supplementary check after
/// independent proof of validity, not the sole trust decision).
fn jwt_audience_unverified(token: &str) -> Option<Vec<String>> {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None; // not a compact JWT
    }

    let decoded = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    aud_claim_to_vec(claims.get("aud")?)
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
            allow_custom_scheme_redirect: false,
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

    fn provider_with_resource_uri(resource_uri: &str) -> OAuth2Provider {
        let config = OAuth2Config {
            client_id: "test-client".to_string(),
            client_secret: "test-secret".to_string().into(),
            auth_url: "https://provider.example.com/oauth/authorize".to_string(),
            token_url: "https://provider.example.com/oauth/token".to_string(),
            revocation_url: None,
            redirect_uri: "http://localhost:8080/callback".to_string(),
            scopes: vec!["openid".to_string()],
            flow_type: crate::config::OAuth2FlowType::AuthorizationCode,
            additional_params: std::collections::HashMap::new(),
            security_level: Default::default(),
            #[cfg(feature = "dpop")]
            dpop_config: None,
            mcp_resource_uri: None,
            auto_resource_indicators: true,
            allow_custom_scheme_redirect: false,
        };
        let oauth_client = OAuth2Client::new(&config, ProviderType::Generic).expect("OAuth2Client");
        OAuth2Provider::new(
            "test".to_string(),
            Arc::new(oauth_client),
            resource_uri.to_string(),
        )
    }

    /// Builds an unsigned (structurally valid) compact JWT carrying the given
    /// `aud` claim — enough to exercise `jwt_audience_unverified`, which
    /// never checks the signature.
    fn unsigned_jwt_with_aud(aud: serde_json::Value) -> String {
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
        let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"none","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(serde_json::json!({ "aud": aud }).to_string());
        format!("{header}.{payload}.")
    }

    /// AU-3: a token whose aud claim matches the server's resource_uri passes.
    #[tokio::test]
    async fn test_validate_audience_binding_accepts_matching_aud() {
        let provider = provider_with_resource_uri("https://mcp.example.com");
        let token = unsigned_jwt_with_aud(serde_json::json!("https://mcp.example.com"));

        assert!(provider.validate_audience_binding(&token).await.is_ok());
    }

    /// AU-3: a token issued for a *different* resource must be rejected —
    /// this is the confused-deputy case RFC 8707 exists to prevent.
    #[tokio::test]
    async fn test_validate_audience_binding_rejects_mismatched_aud() {
        let provider = provider_with_resource_uri("https://mcp.example.com");
        let token = unsigned_jwt_with_aud(serde_json::json!("https://other-server.example.com"));

        assert!(provider.validate_audience_binding(&token).await.is_err());
    }

    /// AU-3: an array-valued aud claim matches if any entry matches.
    #[tokio::test]
    async fn test_validate_audience_binding_accepts_matching_aud_in_array() {
        let provider = provider_with_resource_uri("https://mcp.example.com");
        let token = unsigned_jwt_with_aud(serde_json::json!([
            "https://other.example.com",
            "https://mcp.example.com"
        ]));

        assert!(provider.validate_audience_binding(&token).await.is_ok());
    }

    /// AU-3: an opaque (non-JWT) token with no introspection configured
    /// can't have its audience determined — must fail closed, not skip the check.
    #[tokio::test]
    async fn test_validate_audience_binding_rejects_opaque_token_without_introspection() {
        let provider = provider_with_resource_uri("https://mcp.example.com");

        assert!(
            provider
                .validate_audience_binding("opaque-token-abc123")
                .await
                .is_err()
        );
    }

    /// AU-3: a misconfigured provider (no resource_uri) must fail closed with
    /// a clear config error, not silently skip audience validation.
    #[tokio::test]
    async fn test_validate_audience_binding_fails_closed_without_resource_uri() {
        let provider = provider_with_resource_uri("");
        let token = unsigned_jwt_with_aud(serde_json::json!("https://mcp.example.com"));

        let err = provider
            .validate_audience_binding(&token)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("resource_uri"));
    }
}
