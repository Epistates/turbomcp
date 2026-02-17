//! OAuth 2.1 Authorization Server for WASM MCP servers.
//!
//! This module provides a complete OAuth 2.1 authorization server implementation
//! for Cloudflare Workers and other WASM environments.
//!
//! # ⚠️ DEMO MODE WARNING
//!
//! **This implementation is for demonstration and development purposes only.**
//!
//! The current implementation auto-approves all authorization requests with a
//! hardcoded `"demo-user"` subject. For production use, you MUST:
//!
//! 1. Implement proper user authentication (e.g., redirect to login page)
//! 2. Display a consent screen showing requested scopes
//! 3. Only generate authorization codes after explicit user approval
//! 4. Extract the authenticated user's identity for the `subject` claim
//!
//! Using this in production without proper authentication will issue tokens
//! to anyone who requests them, defeating the purpose of OAuth.
//!
//! # SECURITY: Compile-Time Check
//!
//! When building in release mode, a compile warning will be emitted if this
//! module is used. You MUST implement proper authentication before deploying.
//!
//! # Features
//!
//! - Authorization Code Grant with PKCE (RFC 7636)
//! - Token refresh with dual-token resilience
//! - Token revocation (RFC 7009)
//! - Token introspection (RFC 7662)
//! - Discovery endpoints (RFC 8414, RFC 9728)
//!
//! # Example
//!
//! ```ignore
//! use turbomcp_wasm::auth::provider::{OAuthProvider, OAuthProviderConfig, ClientConfig};
//!
//! let config = OAuthProviderConfig::new("https://my-mcp-server.workers.dev")
//!     .with_client(ClientConfig::public(
//!         "my-client-id",
//!         vec!["https://app.example.com/callback"],
//!     ));
//!
//! let oauth = OAuthProvider::new(config);
//!
//! // In your worker:
//! #[event(fetch)]
//! async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
//!     let url = req.url()?;
//!     let path = url.path();
//!
//!     // Handle OAuth endpoints
//!     if path.starts_with("/oauth/") || path.starts_with("/.well-known/") {
//!         return oauth.handle(req).await;
//!     }
//!
//!     // Handle MCP endpoints (with token validation)
//!     let server = MyMcpServer::new();
//!     server.handle(req).await
//! }
//! ```
//!
//! # Security
//!
//! - PKCE is mandatory for public clients (RFC 9207)
//! - Tokens are stored by hash only (never plaintext)
//! - Constant-time comparison for secrets
//! - Single-use authorization codes

// Security: Emit compile warning in release builds about demo-user (unless demo-oauth feature enabled)
#[cfg(all(not(debug_assertions), not(test), not(feature = "demo-oauth")))]
compile_error!(
    "⚠️  SECURITY WARNING: OAuthProvider uses demo auto-approval by default. \
     You MUST configure a UserAuthenticator before deploying to production, or enable \
     the 'demo-oauth' feature flag to explicitly opt into demo mode. See module documentation for details."
);

mod crypto;
mod storage;
mod types;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::Deserialize;
use worker::{Headers, Request, Response, Url};

pub use crypto::{
    CryptoError, CryptoResult, constant_time_compare, generate_authorization_code,
    generate_code_challenge, generate_family_id, generate_refresh_token, generate_token,
    hash_token, now_secs, validate_code_verifier, verify_pkce,
};
pub use storage::{
    AccessTokenData, AuthorizationCodeGrant, MemoryTokenStore, RefreshTokenData, SharedTokenStore,
    StorageError, StorageResult, TokenStore,
};
pub use types::{
    ClientAuthMethod, ClientConfig, CodeChallengeMethod, GrantType, IntrospectionResponse,
    OAuthError, OAuthProviderConfig, ResponseType, TokenResponse,
};

/// Authenticated user information from the authentication gate.
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    /// Subject identifier (user ID)
    pub subject: String,
    /// Display name for consent screen (optional)
    pub display_name: Option<String>,
}

/// User authentication trait for OAuth authorization endpoint.
///
/// Implement this trait to provide proper user authentication for the OAuth
/// authorization flow. This replaces the demo auto-approval behavior.
///
/// # Example
///
/// ```ignore
/// struct SessionAuthenticator {
///     // Your session management implementation
/// }
///
/// impl UserAuthenticator for SessionAuthenticator {
///     fn authenticate<'a>(
///         &'a self,
///         req: &'a Request,
///     ) -> Pin<Box<dyn Future<Output = Result<Option<AuthenticatedUser>, worker::Error>> + 'a>> {
///         Box::pin(async move {
///             // Check session cookie, verify with your auth system
///             if let Some(user_id) = self.get_user_from_session(req) {
///                 Ok(Some(AuthenticatedUser {
///                     subject: user_id,
///                     display_name: Some("John Doe".to_string()),
///                 }))
///             } else {
///                 Ok(None)
///             }
///         })
///     }
///
///     fn login_redirect(&self, return_to: &str) -> worker::Result<Response> {
///         let login_url = format!("/login?return_to={}", urlencoding::encode(return_to));
///         let headers = Headers::new();
///         headers.set("Location", &login_url)?;
///         Response::empty()?.with_status(302).with_headers(headers)
///     }
/// }
/// ```
pub trait UserAuthenticator: 'static {
    /// Authenticate the user from the incoming request (e.g., via session cookie).
    ///
    /// Returns `Ok(Some(user))` if the user is authenticated, `Ok(None)` if not authenticated.
    fn authenticate<'a>(
        &'a self,
        req: &'a Request,
    ) -> Pin<Box<dyn Future<Output = Result<Option<AuthenticatedUser>, worker::Error>> + 'a>>;

    /// Return a redirect response to the login page.
    ///
    /// The `return_to` parameter contains the original authorization request URL
    /// that should be redirected to after successful login.
    fn login_redirect(&self, return_to: &str) -> worker::Result<Response>;
}

/// Rate limiting result
#[derive(Debug, Clone)]
pub enum RateLimitResult {
    /// Request is allowed, with remaining count
    Allowed { remaining: u32 },
    /// Rate limit exceeded, with retry-after seconds
    Exceeded { retry_after_secs: u32 },
}

/// Rate limiter trait for OAuth endpoints.
///
/// Implement this trait to provide rate limiting for token and authorization endpoints.
///
/// # Example
///
/// ```ignore
/// struct RedisRateLimiter {
///     // Your Redis client
/// }
///
/// impl RateLimiter for RedisRateLimiter {
///     fn check<'a>(
///         &'a self,
///         key: &'a str,
///         max_requests: u32,
///         window_secs: u32,
///     ) -> Pin<Box<dyn Future<Output = Result<RateLimitResult, worker::Error>> + 'a>> {
///         Box::pin(async move {
///             // Implement sliding window or token bucket algorithm
///             let count = self.redis.incr(key).await?;
///             if count > max_requests {
///                 Ok(RateLimitResult::Exceeded { retry_after_secs: window_secs })
///             } else {
///                 Ok(RateLimitResult::Allowed { remaining: max_requests - count })
///             }
///         })
///     }
/// }
/// ```
pub trait RateLimiter: 'static {
    /// Check if the request is within rate limits.
    ///
    /// # Arguments
    ///
    /// * `key` - Rate limit key (e.g., client IP address)
    /// * `max_requests` - Maximum requests allowed in the window
    /// * `window_secs` - Time window in seconds
    ///
    /// Returns the rate limit result (allowed or exceeded).
    fn check<'a>(
        &'a self,
        key: &'a str,
        max_requests: u32,
        window_secs: u32,
    ) -> Pin<Box<dyn Future<Output = Result<RateLimitResult, worker::Error>> + 'a>>;
}

/// OAuth 2.1 Authorization Server.
///
/// Handles OAuth endpoints for authorization, token issuance, and management.
///
/// # Production Usage
///
/// **IMPORTANT:** The default configuration uses an in-memory token store that
/// loses all tokens on Worker restart (~15-30 minutes). For production:
///
/// 1. Use `with_store()` to provide a durable token store (e.g., Durable Objects)
/// 2. Use `with_user_authenticator()` to implement proper user authentication
/// 3. Use `with_rate_limiter()` to add rate limiting protection
///
/// # Example
///
/// ```ignore
/// let oauth = OAuthProvider::new(config)
///     .with_user_authenticator(Box::new(MyAuthenticator::new()))
///     .with_rate_limiter(Box::new(MyRateLimiter::new()));
/// ```
pub struct OAuthProvider {
    config: OAuthProviderConfig,
    store: SharedTokenStore,
    user_authenticator: Option<Box<dyn UserAuthenticator>>,
    rate_limiter: Option<Box<dyn RateLimiter>>,
}

impl OAuthProvider {
    /// Create a new OAuth provider with the given configuration.
    ///
    /// # Security Warning
    ///
    /// The default configuration uses:
    /// - **In-memory token store** that loses all tokens on Worker restart (~15-30 minutes)
    /// - **No user authentication** - you must add a `UserAuthenticator` for production
    ///
    /// Use `with_store()` and `with_user_authenticator()` before deploying to production.
    pub fn new(config: OAuthProviderConfig) -> Self {
        // Warn about memory store usage
        #[cfg(target_arch = "wasm32")]
        web_sys::console::warn_1(
            &"⚠️  Using MemoryTokenStore - tokens will be lost on Worker restart (~15-30 min). \
              Use with_store() with DurableObjectTokenStore for production."
                .into(),
        );

        Self {
            config,
            store: Arc::new(MemoryTokenStore::new()),
            user_authenticator: None,
            rate_limiter: None,
        }
    }

    /// Create a new OAuth provider with a custom token store.
    ///
    /// Use this with a Durable Object-backed store for production deployments.
    pub fn with_store(mut self, store: SharedTokenStore) -> Self {
        self.store = store;
        self
    }

    /// Set the user authenticator for the authorization endpoint.
    ///
    /// **Required for production.** Without this, authorization requests will
    /// return 501 Not Implemented (unless the `demo-oauth` feature is enabled).
    pub fn with_user_authenticator(mut self, authenticator: Box<dyn UserAuthenticator>) -> Self {
        self.user_authenticator = Some(authenticator);
        self
    }

    /// Set the rate limiter for token and authorization endpoints.
    ///
    /// Recommended for production to prevent abuse.
    pub fn with_rate_limiter(mut self, rate_limiter: Box<dyn RateLimiter>) -> Self {
        self.rate_limiter = Some(rate_limiter);
        self
    }

    /// Get the provider configuration.
    pub fn config(&self) -> &OAuthProviderConfig {
        &self.config
    }

    /// Handle an incoming request.
    ///
    /// Routes to the appropriate OAuth endpoint handler.
    pub async fn handle(&self, req: Request) -> worker::Result<Response> {
        let url = req.url()?;
        let path = url.path();

        match (req.method(), path) {
            // Authorization endpoint
            (worker::Method::Get, p) if p == self.config.authorization_endpoint => {
                self.handle_authorize(req).await
            }

            // Token endpoint
            (worker::Method::Post, p) if p == self.config.token_endpoint => {
                self.handle_token(req).await
            }

            // Revocation endpoint
            (worker::Method::Post, p) if p == self.config.revocation_endpoint => {
                self.handle_revoke(req).await
            }

            // Introspection endpoint
            (worker::Method::Post, p) if p == self.config.introspection_endpoint => {
                self.handle_introspect(req).await
            }

            // Discovery: Authorization Server Metadata (RFC 8414)
            (worker::Method::Get, "/.well-known/oauth-authorization-server") => {
                self.handle_authorization_server_metadata()
            }

            // Discovery: Protected Resource Metadata (RFC 9728)
            (worker::Method::Get, "/.well-known/oauth-protected-resource") => {
                self.handle_protected_resource_metadata()
            }

            // JWKS endpoint (placeholder - requires key management)
            (worker::Method::Get, p) if p == self.config.jwks_endpoint => self.handle_jwks(),

            // Unknown endpoint
            _ => self.error_response(404, "Not Found"),
        }
    }

    // =========================================================================
    // Authorization Endpoint
    // =========================================================================

    async fn handle_authorize(&self, req: Request) -> worker::Result<Response> {
        // Rate limit authorization requests (if rate limiter configured)
        if let Some(ref limiter) = self.rate_limiter {
            if let Some(ip) = self.extract_client_ip(&req) {
                match limiter
                    .check(&format!("oauth:authorize:{}", ip), 20, 60)
                    .await?
                {
                    RateLimitResult::Exceeded { retry_after_secs } => {
                        let headers = Headers::new();
                        let _ = headers.set("Retry-After", &retry_after_secs.to_string());
                        return Response::error("Too many requests", 429)
                            .map(|r| r.with_headers(headers));
                    }
                    RateLimitResult::Allowed { .. } => {}
                }
            }
        }

        let url = req.url()?;
        let params = Self::parse_query_params(&url);

        // Extract required parameters
        let client_id = match params.get("client_id") {
            Some(id) => id.clone(),
            None => return self.authorization_error("invalid_request", "Missing client_id", None),
        };

        let redirect_uri = match params.get("redirect_uri") {
            Some(uri) => uri.clone(),
            None => {
                return self.authorization_error("invalid_request", "Missing redirect_uri", None);
            }
        };

        // Validate client
        let client = match self.config.get_client(&client_id) {
            Some(c) => c,
            None => return self.authorization_error("unauthorized_client", "Unknown client", None),
        };

        // Validate redirect URI
        if !client.is_redirect_uri_allowed(&redirect_uri) {
            return self.authorization_error("invalid_request", "Invalid redirect_uri", None);
        }

        let state = params.get("state").cloned();

        // Validate response type
        let response_type = params.get("response_type").map(|s| s.as_str());
        if response_type != Some("code") {
            return self.authorization_redirect_error(
                &redirect_uri,
                "unsupported_response_type",
                "Only 'code' response type is supported",
                state.as_deref(),
            );
        }

        // Validate PKCE for public clients
        let code_challenge = params.get("code_challenge").cloned();
        let code_challenge_method = params
            .get("code_challenge_method")
            .cloned()
            .unwrap_or_else(|| "S256".to_string());

        if client.pkce_required && code_challenge.is_none() {
            return self.authorization_redirect_error(
                &redirect_uri,
                "invalid_request",
                "PKCE code_challenge required",
                state.as_deref(),
            );
        }

        // Parse and validate scopes
        let requested_scopes: Vec<String> = params
            .get("scope")
            .map(|s| s.split_whitespace().map(String::from).collect())
            .unwrap_or_default();

        let scopes = client.effective_scopes(&requested_scopes);

        // =====================================================================
        // AUTHENTICATION GATE: Verify user is authenticated
        // =====================================================================

        let authenticated_user = if let Some(ref authenticator) = self.user_authenticator {
            // Use configured authenticator
            match authenticator.authenticate(&req).await? {
                Some(user) => user,
                None => {
                    // User not authenticated - redirect to login
                    let request_url = url.to_string();
                    return authenticator.login_redirect(&request_url);
                }
            }
        } else {
            // No authenticator configured
            #[cfg(feature = "demo-oauth")]
            {
                // Demo mode - auto-approve with hardcoded user
                #[cfg(target_arch = "wasm32")]
                web_sys::console::warn_1(
                    &"⚠️  OAuth demo mode: auto-approving request with 'demo-user'. \
                      DO NOT USE IN PRODUCTION."
                        .into(),
                );
                AuthenticatedUser {
                    subject: "demo-user".to_string(),
                    display_name: Some("Demo User".to_string()),
                }
            }

            #[cfg(not(feature = "demo-oauth"))]
            {
                // Production mode - require authenticator
                return Response::error(
                    "OAuth provider requires a UserAuthenticator to be configured. \
                     Use with_user_authenticator() to set up proper authentication, or enable \
                     the 'demo-oauth' feature flag for development (NOT for production).",
                    501,
                );
            }
        };

        // Generate authorization code
        let code = match generate_authorization_code() {
            Ok(c) => c,
            Err(e) => {
                return self.authorization_redirect_error(
                    &redirect_uri,
                    "server_error",
                    &format!("Failed to generate code: {}", e),
                    state.as_deref(),
                );
            }
        };

        // Hash the code for storage
        let code_hash = match hash_token(&code).await {
            Ok(h) => h,
            Err(e) => {
                return self.authorization_redirect_error(
                    &redirect_uri,
                    "server_error",
                    &format!("Failed to hash code: {}", e),
                    state.as_deref(),
                );
            }
        };

        // Create grant with authenticated user's subject
        let grant = AuthorizationCodeGrant {
            client_id: client_id.clone(),
            redirect_uri: redirect_uri.clone(),
            scopes,
            code_challenge,
            code_challenge_method: Some(code_challenge_method),
            subject: authenticated_user.subject,
            expires_at: now_secs() + self.config.authorization_code_lifetime,
            nonce: params.get("nonce").cloned(),
            state: state.clone(),
        };

        // Store the grant
        if let Err(e) = self
            .store
            .store_authorization_code(&code_hash, &grant)
            .await
        {
            return self.authorization_redirect_error(
                &redirect_uri,
                "server_error",
                &format!("Failed to store code: {}", e),
                state.as_deref(),
            );
        }

        // Redirect with code
        let mut redirect_url = redirect_uri.clone();
        redirect_url.push_str(if redirect_url.contains('?') { "&" } else { "?" });
        redirect_url.push_str(&format!("code={}", urlencoding::encode(&code)));

        if let Some(ref s) = state {
            redirect_url.push_str(&format!("&state={}", urlencoding::encode(s)));
        }

        self.redirect_response(&redirect_url)
    }

    // =========================================================================
    // Token Endpoint
    // =========================================================================

    async fn handle_token(&self, mut req: Request) -> worker::Result<Response> {
        // Rate limit token requests (if rate limiter configured)
        if let Some(ref limiter) = self.rate_limiter {
            if let Some(ip) = self.extract_client_ip(&req) {
                match limiter
                    .check(&format!("oauth:token:{}", ip), 10, 60)
                    .await?
                {
                    RateLimitResult::Exceeded { retry_after_secs } => {
                        let headers = Headers::new();
                        let _ = headers.set("Retry-After", &retry_after_secs.to_string());
                        return Response::error("Too many requests", 429)
                            .map(|r| r.with_headers(headers));
                    }
                    RateLimitResult::Allowed { .. } => {}
                }
            }
        }

        // Parse form body
        let body = req.text().await?;
        let params = Self::parse_form_params(&body);

        let grant_type = match params.get("grant_type") {
            Some(gt) => gt.as_str(),
            None => {
                return self
                    .token_error_response(OAuthError::invalid_request("Missing grant_type"));
            }
        };

        match grant_type {
            "authorization_code" => self.handle_authorization_code_grant(&params, &req).await,
            "refresh_token" => self.handle_refresh_token_grant(&params, &req).await,
            _ => self.token_error_response(OAuthError::unsupported_grant_type(format!(
                "Unsupported grant type: {}",
                grant_type
            ))),
        }
    }

    async fn handle_authorization_code_grant(
        &self,
        params: &HashMap<String, String>,
        req: &Request,
    ) -> worker::Result<Response> {
        // Extract parameters
        let code = match params.get("code") {
            Some(c) => c.clone(),
            None => return self.token_error_response(OAuthError::invalid_request("Missing code")),
        };

        let redirect_uri = match params.get("redirect_uri") {
            Some(uri) => uri.clone(),
            None => {
                return self
                    .token_error_response(OAuthError::invalid_request("Missing redirect_uri"));
            }
        };

        // Get client (authenticate if confidential)
        let client_id = match self.authenticate_client(params, req)? {
            Some(id) => id,
            None => {
                return self.token_error_response(OAuthError::invalid_client(
                    "Client authentication failed",
                ));
            }
        };

        let client = match self.config.get_client(&client_id) {
            Some(c) => c,
            None => return self.token_error_response(OAuthError::invalid_client("Unknown client")),
        };

        // Hash the code and retrieve grant
        let code_hash = match hash_token(&code).await {
            Ok(h) => h,
            Err(e) => return self.internal_server_error("hash_code", e),
        };

        let grant = match self.store.consume_authorization_code(&code_hash).await {
            Ok(g) => g,
            Err(StorageError::NotFound(_)) | Err(StorageError::Expired(_)) => {
                return self
                    .token_error_response(OAuthError::invalid_grant("Invalid or expired code"));
            }
            Err(e) => return self.internal_server_error("consume_code", e),
        };

        // Validate grant
        if grant.client_id != client_id {
            return self.token_error_response(OAuthError::invalid_grant("Client mismatch"));
        }

        if grant.redirect_uri != redirect_uri {
            return self.token_error_response(OAuthError::invalid_grant("Redirect URI mismatch"));
        }

        // Verify PKCE
        if let Some(ref challenge) = grant.code_challenge {
            let verifier = match params.get("code_verifier") {
                Some(v) => v,
                None => {
                    return self.token_error_response(OAuthError::invalid_request(
                        "Missing code_verifier",
                    ));
                }
            };

            // Validate verifier format
            if !validate_code_verifier(verifier) {
                return self.token_error_response(OAuthError::invalid_grant(
                    "Invalid code_verifier format",
                ));
            }

            let method = grant.code_challenge_method.as_deref().unwrap_or("S256");

            match verify_pkce(verifier, challenge, method).await {
                Ok(true) => {}
                Ok(false) => {
                    return self.token_error_response(OAuthError::invalid_grant(
                        "PKCE verification failed",
                    ));
                }
                Err(e) => return self.internal_server_error("pkce_verify", e),
            }
        } else if client.pkce_required {
            return self
                .token_error_response(OAuthError::invalid_grant("PKCE required but not used"));
        }

        // Generate tokens
        self.issue_tokens(&grant.subject, &client_id, &grant.scopes)
            .await
    }

    async fn handle_refresh_token_grant(
        &self,
        params: &HashMap<String, String>,
        req: &Request,
    ) -> worker::Result<Response> {
        let refresh_token = match params.get("refresh_token") {
            Some(t) => t.clone(),
            None => {
                return self
                    .token_error_response(OAuthError::invalid_request("Missing refresh_token"));
            }
        };

        // Authenticate client
        let client_id = match self.authenticate_client(params, req)? {
            Some(id) => id,
            None => {
                return self.token_error_response(OAuthError::invalid_client(
                    "Client authentication failed",
                ));
            }
        };

        // Hash and retrieve refresh token
        let token_hash = match hash_token(&refresh_token).await {
            Ok(h) => h,
            Err(e) => return self.internal_server_error("hash_refresh_token", e),
        };

        let token_data = match self.store.get_refresh_token(&token_hash).await {
            Ok(d) => d,
            Err(StorageError::NotFound(_)) | Err(StorageError::Expired(_)) => {
                return self.token_error_response(OAuthError::invalid_grant(
                    "Invalid or expired refresh token",
                ));
            }
            Err(e) => return self.internal_server_error("get_refresh_token", e),
        };

        // Validate client
        if token_data.client_id != client_id {
            return self.token_error_response(OAuthError::invalid_grant("Client mismatch"));
        }

        // Check for token reuse (single-use enforcement)
        if token_data.used {
            // Token reuse detected - revoke entire family (security measure)
            let _ = self
                .store
                .revoke_refresh_token_family(&token_data.family_id)
                .await;
            return self.token_error_response(OAuthError::invalid_grant(
                "Refresh token has already been used. All tokens in this family have been revoked.",
            ));
        }

        // Mark token as used
        if let Err(e) = self.store.mark_refresh_token_used(&token_hash).await {
            return self.internal_server_error("mark_token_used", e);
        }

        // Issue new tokens (with same family, incremented generation)
        self.issue_tokens_with_family(
            &token_data.subject,
            &client_id,
            &token_data.scopes,
            &token_data.family_id,
            token_data.generation + 1,
        )
        .await
    }

    // =========================================================================
    // Token Issuance
    // =========================================================================

    async fn issue_tokens(
        &self,
        subject: &str,
        client_id: &str,
        scopes: &[String],
    ) -> worker::Result<Response> {
        let family_id = match generate_family_id() {
            Ok(id) => id,
            Err(e) => return self.internal_server_error("generate_family_id", e),
        };

        self.issue_tokens_with_family(subject, client_id, scopes, &family_id, 0)
            .await
    }

    async fn issue_tokens_with_family(
        &self,
        subject: &str,
        client_id: &str,
        scopes: &[String],
        family_id: &str,
        generation: u32,
    ) -> worker::Result<Response> {
        let now = now_secs();

        // Generate access token (in a real implementation, this would be a JWT)
        let access_token = match generate_token(32) {
            Ok(t) => t,
            Err(e) => return self.internal_server_error("generate_access_token", e),
        };

        let access_token_hash = match hash_token(&access_token).await {
            Ok(h) => h,
            Err(e) => return self.internal_server_error("hash_access_token", e),
        };

        // Store access token metadata
        let access_data = AccessTokenData {
            subject: subject.to_string(),
            client_id: client_id.to_string(),
            scopes: scopes.to_vec(),
            expires_at: now + self.config.access_token_lifetime,
            issued_at: now,
            refresh_token_hash: None,
        };

        if let Err(e) = self
            .store
            .store_access_token(&access_token_hash, &access_data)
            .await
        {
            return self.internal_server_error("store_access_token", e);
        }

        // Generate refresh token if enabled
        let refresh_token = if self.config.issue_refresh_tokens {
            let token = match generate_refresh_token() {
                Ok(t) => t,
                Err(e) => return self.internal_server_error("generate_refresh_token", e),
            };

            let token_hash = match hash_token(&token).await {
                Ok(h) => h,
                Err(e) => return self.internal_server_error("hash_new_refresh_token", e),
            };

            let refresh_data = RefreshTokenData {
                subject: subject.to_string(),
                client_id: client_id.to_string(),
                scopes: scopes.to_vec(),
                expires_at: now + self.config.refresh_token_lifetime,
                issued_at: now,
                generation,
                family_id: family_id.to_string(),
                used: false,
            };

            if let Err(e) = self
                .store
                .store_refresh_token(&token_hash, &refresh_data)
                .await
            {
                return self.internal_server_error("store_refresh_token", e);
            }

            Some(token)
        } else {
            None
        };

        // Build response
        let mut response = TokenResponse::new(&access_token)
            .with_expires_in(self.config.access_token_lifetime)
            .with_scope(scopes);

        if let Some(rt) = refresh_token {
            response = response.with_refresh_token(rt);
        }

        self.json_response(&response)
    }

    // =========================================================================
    // Revocation and Introspection
    // =========================================================================

    async fn handle_revoke(&self, mut req: Request) -> worker::Result<Response> {
        let body = req.text().await?;
        let params = Self::parse_form_params(&body);

        let token = match params.get("token") {
            Some(t) => t.clone(),
            None => {
                return self
                    .token_error_response(OAuthError::invalid_request("Missing token parameter"));
            }
        };

        // Authenticate client
        if self.authenticate_client(&params, &req)?.is_none() {
            return self
                .token_error_response(OAuthError::invalid_client("Client authentication failed"));
        }

        // Hash and try to revoke
        let token_hash = match hash_token(&token).await {
            Ok(h) => h,
            Err(e) => return self.internal_server_error("hash_revoke_token", e),
        };

        // Try revoking as access token
        let _ = self.store.revoke_access_token(&token_hash).await;

        // Try revoking as refresh token (and its family)
        if let Ok(data) = self.store.get_refresh_token(&token_hash).await {
            let _ = self
                .store
                .revoke_refresh_token_family(&data.family_id)
                .await;
        }

        // Per RFC 7009, always return 200 OK
        Response::empty().map(|r| r.with_status(200))
    }

    async fn handle_introspect(&self, mut req: Request) -> worker::Result<Response> {
        let body = req.text().await?;
        let params = Self::parse_form_params(&body);

        let token = match params.get("token") {
            Some(t) => t.clone(),
            None => {
                return self
                    .token_error_response(OAuthError::invalid_request("Missing token parameter"));
            }
        };

        // Authenticate client
        if self.authenticate_client(&params, &req)?.is_none() {
            return self
                .token_error_response(OAuthError::invalid_client("Client authentication failed"));
        }

        // Hash and lookup
        let token_hash = match hash_token(&token).await {
            Ok(h) => h,
            Err(e) => return self.internal_server_error("hash_introspect_token", e),
        };

        // Try as access token first
        if let Ok(data) = self.store.get_access_token(&token_hash).await {
            let response = IntrospectionResponse::active(
                &data.subject,
                &data.client_id,
                &data.scopes,
                data.expires_at,
                data.issued_at,
            );
            return self.json_response(&response);
        }

        // Try as refresh token
        if let Ok(data) = self.store.get_refresh_token(&token_hash).await {
            if !data.used {
                let response = IntrospectionResponse::active(
                    &data.subject,
                    &data.client_id,
                    &data.scopes,
                    data.expires_at,
                    data.issued_at,
                );
                return self.json_response(&response);
            }
        }

        // Token not found or inactive
        self.json_response(&IntrospectionResponse::inactive())
    }

    // =========================================================================
    // Discovery Endpoints
    // =========================================================================

    fn handle_authorization_server_metadata(&self) -> worker::Result<Response> {
        let metadata = serde_json::json!({
            "issuer": self.config.issuer,
            "authorization_endpoint": self.config.authorization_endpoint_url(),
            "token_endpoint": self.config.token_endpoint_url(),
            "revocation_endpoint": format!("{}{}", self.config.issuer, self.config.revocation_endpoint),
            "introspection_endpoint": format!("{}{}", self.config.issuer, self.config.introspection_endpoint),
            "jwks_uri": self.config.jwks_endpoint_url(),
            "response_types_supported": ["code"],
            "grant_types_supported": ["authorization_code", "refresh_token"],
            "token_endpoint_auth_methods_supported": ["none", "client_secret_basic", "client_secret_post"],
            "code_challenge_methods_supported": ["S256", "plain"],
            "scopes_supported": self.config.supported_scopes,
        });

        self.json_response(&metadata)
    }

    fn handle_protected_resource_metadata(&self) -> worker::Result<Response> {
        let metadata = serde_json::json!({
            "resource": self.config.issuer,
            "authorization_servers": [self.config.issuer],
            "bearer_methods_supported": ["header"],
            "scopes_supported": self.config.supported_scopes,
        });

        self.json_response(&metadata)
    }

    fn handle_jwks(&self) -> worker::Result<Response> {
        // Placeholder - in a real implementation, this would return the signing keys
        let jwks = serde_json::json!({
            "keys": []
        });

        self.json_response(&jwks)
    }

    // =========================================================================
    // Client Authentication
    // =========================================================================

    fn authenticate_client(
        &self,
        params: &HashMap<String, String>,
        req: &Request,
    ) -> worker::Result<Option<String>> {
        // Try client_id/client_secret from body
        if let Some(client_id) = params.get("client_id") {
            let client = match self.config.get_client(client_id) {
                Some(c) => c,
                None => return Ok(None),
            };

            match client.auth_method {
                ClientAuthMethod::None => {
                    // Public client - no secret required
                    return Ok(Some(client_id.clone()));
                }
                ClientAuthMethod::ClientSecretPost => {
                    if let Some(secret) = params.get("client_secret") {
                        if let Some(ref expected) = client.client_secret {
                            if constant_time_compare(secret, expected) {
                                return Ok(Some(client_id.clone()));
                            }
                        }
                    }
                    return Ok(None);
                }
                ClientAuthMethod::ClientSecretBasic => {
                    // Continue to try Basic auth header
                }
            }
        }

        // Try Basic auth header
        if let Ok(Some(auth)) = req.headers().get("Authorization") {
            if let Some(credentials) = auth.strip_prefix("Basic ") {
                if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(credentials) {
                    if let Ok(creds_str) = String::from_utf8(decoded) {
                        if let Some((client_id, client_secret)) = creds_str.split_once(':') {
                            if let Some(client) = self.config.get_client(client_id) {
                                if let Some(ref expected) = client.client_secret {
                                    if constant_time_compare(client_secret, expected) {
                                        return Ok(Some(client_id.to_string()));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // For public clients, just check client_id exists
        if let Some(client_id) = params.get("client_id") {
            if let Some(client) = self.config.get_client(client_id) {
                if matches!(client.auth_method, ClientAuthMethod::None) {
                    return Ok(Some(client_id.clone()));
                }
            }
        }

        Ok(None)
    }

    // =========================================================================
    // Helper Methods
    // =========================================================================

    /// Extract client IP address from request headers.
    ///
    /// Cloudflare Workers provide the client IP in the CF-Connecting-IP header.
    fn extract_client_ip(&self, req: &Request) -> Option<String> {
        req.headers()
            .get("CF-Connecting-IP")
            .ok()
            .flatten()
            .or_else(|| req.headers().get("X-Forwarded-For").ok().flatten())
            .or_else(|| req.headers().get("X-Real-IP").ok().flatten())
    }

    // =========================================================================
    // Response Helpers
    // =========================================================================

    fn parse_query_params(url: &Url) -> HashMap<String, String> {
        url.query_pairs()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn parse_form_params(body: &str) -> HashMap<String, String> {
        form_urlencoded::parse(body.as_bytes())
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn json_response<T: serde::Serialize>(&self, body: &T) -> worker::Result<Response> {
        let json = serde_json::to_string(body).map_err(|e| worker::Error::from(e.to_string()))?;

        let headers = Headers::new();
        let _ = headers.set("Content-Type", "application/json");
        let _ = headers.set("Cache-Control", "no-store");
        let _ = headers.set("Pragma", "no-cache");

        Ok(Response::ok(json)?.with_headers(headers))
    }

    fn redirect_response(&self, url: &str) -> worker::Result<Response> {
        let headers = Headers::new();
        let _ = headers.set("Location", url);

        Response::empty()
            .map(|r| r.with_status(302))
            .map(|r| r.with_headers(headers))
    }

    fn error_response(&self, status: u16, message: &str) -> worker::Result<Response> {
        Response::error(message, status)
    }

    fn authorization_error(
        &self,
        error: &str,
        description: &str,
        state: Option<&str>,
    ) -> worker::Result<Response> {
        let mut err = OAuthError {
            error: error.to_string(),
            error_description: Some(description.to_string()),
            error_uri: None,
            state: state.map(String::from),
        };

        // For authorization errors without redirect_uri, return JSON
        self.json_response(&err).map(|r| r.with_status(400))
    }

    fn authorization_redirect_error(
        &self,
        redirect_uri: &str,
        error: &str,
        description: &str,
        state: Option<&str>,
    ) -> worker::Result<Response> {
        let mut url = redirect_uri.to_string();
        url.push_str(if url.contains('?') { "&" } else { "?" });
        url.push_str(&format!("error={}", urlencoding::encode(error)));
        url.push_str(&format!(
            "&error_description={}",
            urlencoding::encode(description)
        ));
        if let Some(s) = state {
            url.push_str(&format!("&state={}", urlencoding::encode(s)));
        }

        self.redirect_response(&url)
    }

    fn token_error_response(&self, error: OAuthError) -> worker::Result<Response> {
        self.json_response(&error).map(|r| r.with_status(400))
    }

    /// Return a generic server error response while logging the internal error.
    ///
    /// SECURITY: This method prevents internal error details from leaking to clients.
    /// The detailed error is logged for operators but a generic message is returned.
    #[cfg(target_arch = "wasm32")]
    fn internal_server_error(
        &self,
        context: &str,
        error: impl std::fmt::Display,
    ) -> worker::Result<Response> {
        // Log the internal error for operators
        web_sys::console::error_1(&format!("OAuth internal error ({}): {}", context, error).into());
        // Return generic error to client
        self.token_error_response(OAuthError::server_error("An internal error occurred"))
    }

    /// Return a generic server error response while logging the internal error.
    ///
    /// SECURITY: This method prevents internal error details from leaking to clients.
    /// The detailed error is logged for operators but a generic message is returned.
    #[cfg(not(target_arch = "wasm32"))]
    fn internal_server_error(
        &self,
        context: &str,
        error: impl std::fmt::Display,
    ) -> worker::Result<Response> {
        // Log the internal error for operators
        eprintln!("OAuth internal error ({}): {}", context, error);
        // Return generic error to client
        self.token_error_response(OAuthError::server_error("An internal error occurred"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_config() {
        let config = OAuthProviderConfig::new("https://my-server.workers.dev")
            .with_client(ClientConfig::public(
                "test-client",
                vec!["https://app.example.com/callback".to_string()],
            ))
            .with_scopes(vec!["read".to_string(), "write".to_string()]);

        assert_eq!(config.issuer, "https://my-server.workers.dev");
        assert!(config.get_client("test-client").is_some());
        assert!(config.get_client("unknown").is_none());
    }

    #[test]
    fn test_endpoint_urls() {
        let config = OAuthProviderConfig::new("https://my-server.workers.dev");

        assert_eq!(
            config.authorization_endpoint_url(),
            "https://my-server.workers.dev/oauth/authorize"
        );
        assert_eq!(
            config.token_endpoint_url(),
            "https://my-server.workers.dev/oauth/token"
        );
        assert_eq!(
            config.jwks_endpoint_url(),
            "https://my-server.workers.dev/.well-known/jwks.json"
        );
    }
}
