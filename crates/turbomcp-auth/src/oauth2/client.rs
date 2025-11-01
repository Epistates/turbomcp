//! OAuth 2.1 Client Implementation
//!
//! This module provides an OAuth 2.1 client wrapper that supports:
//! - Authorization Code flow (with PKCE)
//! - Client Credentials flow (server-to-server)
//! - Device Authorization flow (CLI/IoT)
//!
//! The client handles provider-specific configurations and quirks for
//! Google, Microsoft, GitHub, GitLab, and generic OAuth providers.

use std::collections::HashMap;

use oauth2::{
    AuthUrl, ClientId, ClientSecret, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl,
    RefreshToken, RevocationUrl, Scope, TokenResponse, TokenUrl,
    basic::{BasicClient, BasicTokenType},
    revocation::StandardRevocableToken,
};
use secrecy::ExposeSecret;

use turbomcp_protocol::{Error as McpError, Result as McpResult};

use super::super::config::{OAuth2Config, ProviderConfig, ProviderType, RefreshBehavior};
use super::super::types::TokenInfo;

/// OAuth 2.1 client wrapper supporting all modern flows
#[derive(Debug, Clone)]
pub struct OAuth2Client {
    /// Authorization code flow client (most common)
    pub(crate) auth_code_client: BasicClient,
    /// Client credentials client (server-to-server)
    pub(crate) client_credentials_client: Option<BasicClient>,
    /// Device code client (for CLI/IoT applications)
    pub(crate) device_code_client: Option<BasicClient>,
    /// Provider-specific configuration
    pub provider_config: ProviderConfig,
}

impl OAuth2Client {
    /// Create an OAuth 2.1 client supporting all flows
    pub fn new(config: &OAuth2Config, provider_type: ProviderType) -> McpResult<Self> {
        // Validate URLs
        let auth_url = AuthUrl::new(config.auth_url.clone())
            .map_err(|_| McpError::validation("Invalid authorization URL".to_string()))?;

        let token_url = TokenUrl::new(config.token_url.clone())
            .map_err(|_| McpError::validation("Invalid token URL".to_string()))?;

        // Redirect URI validation with security checks
        let redirect_url = Self::validate_redirect_uri(&config.redirect_uri)?;

        // Create authorization code flow client (primary)
        let client_secret = if config.client_secret.expose_secret().is_empty() {
            None
        } else {
            Some(ClientSecret::new(
                config.client_secret.expose_secret().clone(),
            ))
        };

        let mut auth_code_client = BasicClient::new(
            ClientId::new(config.client_id.clone()),
            client_secret.clone(),
            auth_url.clone(),
            Some(token_url.clone()),
        )
        .set_redirect_uri(redirect_url);

        // Set revocation endpoint if provided (RFC 7009)
        if let Some(ref revocation_url_str) = config.revocation_url {
            let revocation_url = RevocationUrl::new(revocation_url_str.clone())
                .map_err(|_| McpError::validation("Invalid revocation URL".to_string()))?;
            auth_code_client = auth_code_client.set_revocation_uri(revocation_url);
        }

        // Create client credentials client if we have a secret (server-to-server)
        let client_credentials_client = if client_secret.is_some() {
            Some(BasicClient::new(
                ClientId::new(config.client_id.clone()),
                client_secret.clone(),
                auth_url.clone(),
                Some(token_url.clone()),
            ))
        } else {
            None
        };

        // Device code client (for CLI/IoT apps) - uses same configuration
        let device_code_client = Some(BasicClient::new(
            ClientId::new(config.client_id.clone()),
            client_secret,
            auth_url,
            Some(token_url),
        ));

        // Provider-specific configuration
        let provider_config = Self::build_provider_config(provider_type);

        Ok(Self {
            auth_code_client,
            client_credentials_client,
            device_code_client,
            provider_config,
        })
    }

    /// Build provider-specific configuration
    fn build_provider_config(provider_type: ProviderType) -> ProviderConfig {
        match provider_type {
            ProviderType::Google => ProviderConfig {
                provider_type,
                default_scopes: vec![
                    "openid".to_string(),
                    "email".to_string(),
                    "profile".to_string(),
                ],
                refresh_behavior: RefreshBehavior::Proactive,
                userinfo_endpoint: Some(
                    "https://www.googleapis.com/oauth2/v2/userinfo".to_string(),
                ),
                additional_params: HashMap::new(),
            },
            ProviderType::Microsoft => ProviderConfig {
                provider_type,
                default_scopes: vec![
                    "openid".to_string(),
                    "profile".to_string(),
                    "email".to_string(),
                    "User.Read".to_string(),
                ],
                refresh_behavior: RefreshBehavior::Proactive,
                userinfo_endpoint: Some("https://graph.microsoft.com/v1.0/me".to_string()),
                additional_params: HashMap::new(),
            },
            ProviderType::GitHub => ProviderConfig {
                provider_type,
                default_scopes: vec!["user:email".to_string(), "read:user".to_string()],
                refresh_behavior: RefreshBehavior::Reactive,
                userinfo_endpoint: Some("https://api.github.com/user".to_string()),
                additional_params: HashMap::new(),
            },
            ProviderType::GitLab => ProviderConfig {
                provider_type,
                default_scopes: vec!["read_user".to_string(), "openid".to_string()],
                refresh_behavior: RefreshBehavior::Proactive,
                userinfo_endpoint: Some("https://gitlab.com/api/v4/user".to_string()),
                additional_params: HashMap::new(),
            },
            ProviderType::Apple => ProviderConfig {
                provider_type,
                default_scopes: vec!["openid".to_string(), "email".to_string(), "name".to_string()],
                refresh_behavior: RefreshBehavior::Proactive,
                userinfo_endpoint: Some("https://appleid.apple.com/auth/v1/user".to_string()),
                additional_params: {
                    let mut params = HashMap::new();
                    // Apple requires response_mode=form_post for web apps
                    params.insert("response_mode".to_string(), "form_post".to_string());
                    params
                },
            },
            ProviderType::Okta => ProviderConfig {
                provider_type,
                default_scopes: vec!["openid".to_string(), "email".to_string(), "profile".to_string()],
                refresh_behavior: RefreshBehavior::Proactive,
                userinfo_endpoint: Some("/oauth2/v1/userinfo".to_string()), // Relative to Okta domain
                additional_params: HashMap::new(),
            },
            ProviderType::Auth0 => ProviderConfig {
                provider_type,
                default_scopes: vec!["openid".to_string(), "email".to_string(), "profile".to_string()],
                refresh_behavior: RefreshBehavior::Proactive,
                userinfo_endpoint: Some("/userinfo".to_string()), // Relative to Auth0 domain
                additional_params: HashMap::new(),
            },
            ProviderType::Keycloak => ProviderConfig {
                provider_type,
                default_scopes: vec!["openid".to_string(), "email".to_string(), "profile".to_string()],
                refresh_behavior: RefreshBehavior::Proactive,
                userinfo_endpoint: Some("/realms/{realm}/protocol/openid-connect/userinfo".to_string()),
                additional_params: HashMap::new(),
            },
            ProviderType::Generic | ProviderType::Custom(_) => ProviderConfig {
                provider_type,
                default_scopes: vec!["openid".to_string(), "profile".to_string()],
                refresh_behavior: RefreshBehavior::Proactive,
                userinfo_endpoint: None,
                additional_params: HashMap::new(),
            },
        }
    }

    /// Redirect URI validation with security checks
    ///
    /// Security considerations:
    /// - Prevents open redirect attacks
    /// - Validates URL format and structure
    /// - Environment-aware validation (localhost for development)
    fn validate_redirect_uri(uri: &str) -> McpResult<RedirectUrl> {
        use url::Url;

        // Parse and validate URL structure
        let parsed = Url::parse(uri)
            .map_err(|e| McpError::validation(format!("Invalid redirect URI format: {e}")))?;

        // Security: Validate scheme
        match parsed.scheme() {
            "http" => {
                // Only allow http for localhost/127.0.0.1/0.0.0.0 in development
                if let Some(host) = parsed.host_str() {
                    // Allow localhost, 127.0.0.1, 0.0.0.0 (bind all interfaces)
                    let is_localhost = host == "localhost"
                        || host.starts_with("localhost:")
                        || host == "127.0.0.1"
                        || host.starts_with("127.0.0.1:")
                        || host == "0.0.0.0"
                        || host.starts_with("0.0.0.0:");

                    if !is_localhost {
                        return Err(McpError::validation(
                            "HTTP redirect URIs only allowed for localhost in development"
                                .to_string(),
                        ));
                    }
                } else {
                    return Err(McpError::validation(
                        "Redirect URI must have a valid host".to_string(),
                    ));
                }
            }
            "https" => {
                // HTTPS is always allowed
            }
            "com.example.app" | "msauth" => {
                // Allow custom schemes for mobile apps (common patterns)
            }
            scheme if scheme.starts_with("app.") || scheme.ends_with(".app") => {
                // Allow app-specific custom schemes
            }
            _ => {
                return Err(McpError::validation(format!(
                    "Unsupported redirect URI scheme: {}. Use https, http (localhost only), or app-specific schemes",
                    parsed.scheme()
                )));
            }
        }

        // Security: Prevent fragment in redirect URI (per OAuth 2.0 spec)
        if parsed.fragment().is_some() {
            return Err(McpError::validation(
                "Redirect URI must not contain URL fragment".to_string(),
            ));
        }

        // Security: Check for path traversal in PATH component only
        // Note: url::Url::parse() already normalizes paths and removes .. segments
        // We check the final path to ensure no traversal remains after normalization
        if let Some(path) = parsed.path_segments() {
            for segment in path {
                if segment == ".." {
                    return Err(McpError::validation(
                        "Redirect URI path must not contain traversal sequences".to_string(),
                    ));
                }
            }
        }

        // Use oauth2 crate's RedirectUrl for validation
        // This provides URL validation per OAuth 2.1 specifications
        // For production security, implement exact whitelist matching of allowed URIs
        RedirectUrl::new(uri.to_string())
            .map_err(|_| McpError::validation("Failed to create redirect URL".to_string()))
    }

    /// Get access to the authorization code client
    #[must_use]
    pub fn auth_code_client(&self) -> &BasicClient {
        &self.auth_code_client
    }

    /// Get access to the client credentials client (if available)
    #[must_use]
    pub fn client_credentials_client(&self) -> Option<&BasicClient> {
        self.client_credentials_client.as_ref()
    }

    /// Get access to the device code client (if available)
    #[must_use]
    pub fn device_code_client(&self) -> Option<&BasicClient> {
        self.device_code_client.as_ref()
    }

    /// Get the provider configuration
    #[must_use]
    pub fn provider_config(&self) -> &ProviderConfig {
        &self.provider_config
    }

    /// Start authorization code flow with PKCE
    ///
    /// This initiates the OAuth 2.1 authorization code flow with PKCE (RFC 7636)
    /// for enhanced security, especially for public clients.
    ///
    /// # PKCE Code Verifier Storage (CRITICAL SECURITY REQUIREMENT)
    ///
    /// The returned code_verifier MUST be securely stored and associated with the
    /// state parameter until the authorization code is exchanged for tokens.
    ///
    /// **Storage Options (from most to least secure):**
    ///
    /// 1. **Server-side encrypted session** (RECOMMENDED for web apps)
    ///    - Store in server session with HttpOnly, Secure, SameSite=Lax cookies
    ///    - Associate with state parameter for CSRF protection
    ///    - Automatic cleanup after exchange or timeout
    ///
    /// 2. **Redis/Database with TTL** (RECOMMENDED for distributed systems)
    ///    - Key: state parameter, Value: encrypted code_verifier
    ///    - Set TTL to match authorization timeout (typically 10 minutes)
    ///    - Use server-side encryption at rest
    ///
    /// 3. **In-memory for SPAs** (ACCEPTABLE for public clients only)
    ///    - Store in JavaScript closure or React state (NOT localStorage/sessionStorage)
    ///    - Clear immediately after token exchange
    ///    - Risk: XSS can steal verifier
    ///
    /// **NEVER:**
    /// - Store in localStorage or sessionStorage (XSS risk)
    /// - Send to client in URL or query parameters
    /// - Log or expose in error messages
    ///
    /// # Arguments
    /// * `scopes` - Requested OAuth scopes
    /// * `state` - CSRF protection state parameter (use cryptographically random value)
    ///
    /// # Returns
    /// Tuple of (authorization_url, PKCE code_verifier for secure storage)
    ///
    /// # Example
    /// ```ignore
    /// // Server-side web app (RECOMMENDED)
    /// let state = generate_csrf_token();  // Cryptographically random
    /// let (auth_url, code_verifier) = client.authorization_code_flow(scopes, state.clone());
    ///
    /// // Store securely server-side
    /// session.insert("oauth_state", state);
    /// session.insert("pkce_verifier", code_verifier);  // Encrypted session
    ///
    /// // Redirect user
    /// redirect_to(auth_url);
    /// ```
    pub fn authorization_code_flow(&self, scopes: Vec<String>, state: String) -> (String, String) {
        // Generate PKCE challenge
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

        // Build authorization URL with PKCE
        let (auth_url, _state) = self
            .auth_code_client
            .authorize_url(|| oauth2::CsrfToken::new(state))
            .add_scopes(scopes.into_iter().map(Scope::new))
            .set_pkce_challenge(pkce_challenge)
            .url();

        (auth_url.to_string(), pkce_verifier.secret().to_string())
    }

    /// Exchange authorization code for access token
    ///
    /// This exchanges the authorization code received from the OAuth provider
    /// for an access token using PKCE (RFC 7636).
    ///
    /// # Arguments
    /// * `code` - Authorization code from OAuth provider
    /// * `code_verifier` - PKCE code verifier (from authorization_code_flow)
    ///
    /// # Returns
    /// TokenInfo containing access token and refresh token (if available)
    pub async fn exchange_code_for_token(
        &self,
        code: String,
        code_verifier: String,
    ) -> McpResult<TokenInfo> {
        let http_client = reqwest::Client::new();
        let token_response = self
            .auth_code_client
            .exchange_code(oauth2::AuthorizationCode::new(code))
            .set_pkce_verifier(PkceCodeVerifier::new(code_verifier))
            .request_async(|request| async { execute_oauth_request(&http_client, request).await })
            .await
            .map_err(|e| McpError::internal(format!("Token exchange failed: {e}")))?;

        Ok(self.token_response_to_token_info(token_response))
    }

    /// Refresh an access token with automatic refresh token rotation
    ///
    /// This uses a refresh token to obtain a new access token without
    /// requiring user interaction. OAuth 2.1 and RFC 9700 recommend refresh
    /// token rotation where the server issues a new refresh token with each
    /// refresh request.
    ///
    /// # Refresh Token Rotation (OAuth 2.1 / RFC 9700 Best Practice)
    ///
    /// When the server supports rotation:
    /// - A new refresh token is returned in the response
    /// - The old refresh token should be discarded immediately
    /// - Store and use the new refresh token for future requests
    /// - This prevents token theft detection
    ///
    /// **Important:** Always check if `token_info.refresh_token` is present in
    /// the response. If present, you MUST replace your stored refresh token
    /// with the new one. If absent, continue using the current refresh token.
    ///
    /// # Arguments
    /// * `refresh_token` - The current refresh token
    ///
    /// # Returns
    /// New TokenInfo with:
    /// - Fresh access token (always present)
    /// - New refresh token (if server supports rotation)
    ///
    /// # Example
    /// ```ignore
    /// let mut stored_refresh_token = "current_refresh_token";
    /// let new_tokens = client.refresh_access_token(stored_refresh_token).await?;
    ///
    /// // Check for refresh token rotation
    /// if let Some(new_refresh_token) = &new_tokens.refresh_token {
    ///     // Server rotated the token - update storage
    ///     stored_refresh_token = new_refresh_token;
    ///     println!("Refresh token rotated (security best practice)");
    /// }
    /// // Use new access token
    /// let access_token = new_tokens.access_token;
    /// ```
    pub async fn refresh_access_token(&self, refresh_token: &str) -> McpResult<TokenInfo> {
        let http_client = reqwest::Client::new();
        let token_response = self
            .auth_code_client
            .exchange_refresh_token(&RefreshToken::new(refresh_token.to_string()))
            .request_async(|request| async { execute_oauth_request(&http_client, request).await })
            .await
            .map_err(|e| McpError::internal(format!("Token refresh failed: {e}")))?;

        Ok(self.token_response_to_token_info(token_response))
    }

    /// Client credentials flow for server-to-server authentication
    ///
    /// This implements the OAuth 2.1 Client Credentials flow for
    /// service-to-service communication without user involvement.
    ///
    /// # Arguments
    /// * `scopes` - Requested OAuth scopes
    ///
    /// # Returns
    /// TokenInfo with access token (typically without refresh token)
    pub async fn client_credentials_flow(&self, scopes: Vec<String>) -> McpResult<TokenInfo> {
        let client = self.client_credentials_client.as_ref().ok_or_else(|| {
            McpError::internal("Client credentials flow requires client secret".to_string())
        })?;

        let http_client = reqwest::Client::new();
        let token_response = client
            .exchange_client_credentials()
            .add_scopes(scopes.into_iter().map(Scope::new))
            .request_async(|request| async { execute_oauth_request(&http_client, request).await })
            .await
            .map_err(|e| McpError::internal(format!("Client credentials flow failed: {e}")))?;

        Ok(self.token_response_to_token_info(token_response))
    }

    /// Convert oauth2 token response to TokenInfo
    fn token_response_to_token_info(
        &self,
        response: oauth2::StandardTokenResponse<oauth2::EmptyExtraTokenFields, BasicTokenType>,
    ) -> TokenInfo {
        let expires_in = response.expires_in().map(|duration| duration.as_secs());

        TokenInfo {
            access_token: response.access_token().secret().clone(),
            token_type: format!("{:?}", response.token_type()),
            refresh_token: response.refresh_token().map(|t| t.secret().clone()),
            expires_in,
            scope: response.scopes().map(|scopes| {
                scopes
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            }),
        }
    }

    /// Revoke a token using RFC 7009 Token Revocation
    ///
    /// Per RFC 7009 Section 2, prefer revoking refresh tokens (which MUST be supported
    /// by the server if issued) over access tokens (which MAY be supported).
    ///
    /// # Arguments
    /// * `token_info` - Token information containing access and/or refresh token
    ///
    /// # Returns
    /// Ok if revocation succeeded or token was already invalid (per RFC 7009)
    ///
    /// # Errors
    /// Returns error if:
    /// - No revocation endpoint was configured
    /// - Network/HTTP error occurred
    /// - Server returned an error response
    pub async fn revoke_token(&self, token_info: &TokenInfo) -> McpResult<()> {
        let http_client = reqwest::Client::new();

        // Per RFC 7009 Section 2: Prefer refresh token, fallback to access token
        let token_to_revoke: StandardRevocableToken =
            if let Some(ref refresh_token) = token_info.refresh_token {
                RefreshToken::new(refresh_token.clone()).into()
            } else {
                oauth2::AccessToken::new(token_info.access_token.clone()).into()
            };

        self.auth_code_client
            .revoke_token(token_to_revoke)
            .map_err(|e| McpError::internal(format!("Token revocation not configured: {e}")))?
            .request_async(|request| async { execute_oauth_request(&http_client, request).await })
            .await
            .map_err(|e| McpError::internal(format!("Token revocation failed: {e}")))?;

        Ok(())
    }

    /// Validate that an access token is still valid
    ///
    /// This checks if a token has expired based on expiration time.
    /// Note: This is a client-side check only; servers may have revoked the token.
    pub fn is_token_expired(&self, token: &TokenInfo) -> bool {
        if let Some(expires_in) = token.expires_in {
            // Assume token was valid "now" - in production, store issued_at timestamp
            expires_in == 0
        } else {
            false
        }
    }
}

/// Execute OAuth request using reqwest HTTP client
/// Converts between oauth2 and reqwest types
async fn execute_oauth_request(
    client: &reqwest::Client,
    request: oauth2::HttpRequest,
) -> Result<oauth2::HttpResponse, oauth2::reqwest::Error<reqwest::Error>> {
    let method_str = format!("{}", request.method);
    let url = request.url.clone();

    // Build the request
    let mut req_builder = match method_str.to_uppercase().as_str() {
        "GET" => client.get(url),
        "POST" => client.post(url),
        m => {
            return Err(oauth2::reqwest::Error::Other(format!(
                "Unsupported HTTP method: {}",
                m
            )));
        }
    };

    // Add body (always present, even if empty)
    if !request.body.is_empty() {
        req_builder = req_builder.body(request.body);
    }

    // Add headers - convert from oauth2 HeaderName/HeaderValue to reqwest types
    for (name, value) in &request.headers {
        let name_str = format!("{:?}", name); // Use debug format for HeaderName
        // HeaderValue as_bytes should work
        let value_bytes = value.as_bytes();

        if let (Ok(header_name), Ok(header_value)) = (
            reqwest::header::HeaderName::from_bytes(name_str.as_bytes()),
            reqwest::header::HeaderValue::from_bytes(value_bytes),
        ) {
            req_builder = req_builder.header(header_name, header_value);
        }
    }

    // Send request
    let response = req_builder
        .send()
        .await
        .map_err(|e| oauth2::reqwest::Error::Other(e.to_string()))?;

    let status = response.status();
    let body = response
        .bytes()
        .await
        .map_err(|e| oauth2::reqwest::Error::Other(e.to_string()))?
        .to_vec();

    // Convert reqwest status code to oauth2 status code
    let oauth_status =
        oauth2::http::StatusCode::from_u16(status.as_u16()).unwrap_or(oauth2::http::StatusCode::OK);

    Ok(oauth2::HttpResponse {
        status_code: oauth_status,
        body,
        headers: Default::default(),
    })
}
