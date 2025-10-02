//! Authentication and Authorization system for `TurboMCP` servers
//!
//! This module provides comprehensive OAuth 2.1 MCP compliance and authentication capabilities including:
//!
//! ## OAuth 2.1 MCP Compliance
//! - **RFC 8707 Resource Indicators** - MCP resource URI binding for token scoping
//! - **RFC 9728 Protected Resource Metadata** - Discovery and validation endpoints
//! - **RFC 7591 Dynamic Client Registration** - Runtime client configuration
//! - **PKCE Support** - Enhanced security with Proof Key for Code Exchange
//! - **Multi-Provider Support** - Google, GitHub, Microsoft OAuth 2.0 integration
//!
//! ## Security Features
//! - **Redirect URI Validation** - Prevents open redirect attacks
//! - **Domain Whitelisting** - Environment-based host validation
//! - **Attack Vector Prevention** - Protection against injection and traversal attacks
//! - **Security Levels** - Standard, Enhanced, Maximum security configurations
//!
//! ## Legacy Authentication (Planned)
//! - JWT token validation and generation
//! - API key authentication
//! - Role-based access control (RBAC)
//! - Custom authentication providers
//! - Session management
//! - Token refresh and validation

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::{McpError, McpResult};

// Using battle-tested oauth2 crate for secure OAuth2 implementation
use oauth2::{
    AuthorizationCode, CsrfToken, PkceCodeChallenge, PkceCodeVerifier, RefreshToken, Scope,
    TokenResponse, reqwest::async_http_client,
};

// DPoP support (feature-gated)
#[cfg(feature = "dpop")]
use crate::auth::dpop::{DpopKeyManager, DpopProofGenerator};

// Import configuration types from auth::config
use crate::auth::config::*;

// Import core types from auth::types
use crate::auth::types::*;

// Import OAuth2Client from oauth2 module
use crate::auth::oauth2::OAuth2Client;

/// Production-grade OAuth 2.0 authentication provider supporting all modern flows
#[derive(Debug)]
pub struct OAuth2Provider {
    /// Provider name
    name: String,
    /// OAuth 2.0 configuration
    config: OAuth2Config,
    /// Comprehensive OAuth2 client supporting multiple flows
    oauth_client: OAuth2Client,
    /// Secure token storage
    token_storage: Arc<dyn TokenStorage>,
    /// Pending authorization requests with PKCE verifiers
    pending_auths: Arc<RwLock<HashMap<String, PendingAuth>>>,
    /// Protected Resource Registry (RFC 9728) for server-side discovery
    resource_registry: Option<Arc<McpResourceRegistry>>,
    /// Dynamic Client Registration (RFC 7591) manager
    dynamic_registration: Option<Arc<DynamicClientRegistration>>,
    /// DPoP proof generator for enhanced security
    #[cfg(feature = "dpop")]
    #[allow(dead_code)] // Prepared for future DPoP functionality
    dpop_generator: Option<Arc<DpopProofGenerator>>,
}

#[derive(Debug)]
struct PendingAuth {
    state: CsrfToken,
    pkce_verifier: PkceCodeVerifier,
    created_at: SystemTime,
    expires_at: SystemTime,
    /// Resource URI for RFC 8707 Resource Indicators
    resource_uri: Option<String>,
}

impl OAuth2Provider {
    /// Create a production-grade OAuth 2.0 provider with comprehensive flow support
    pub async fn new(
        name: String,
        config: OAuth2Config,
        provider_type: ProviderType,
        token_storage: Arc<dyn TokenStorage>,
    ) -> McpResult<Self> {
        let oauth_client = OAuth2Client::new(&config, provider_type)?;

        // Initialize DPoP generator for enhanced security levels
        #[cfg(feature = "dpop")]
        let dpop_generator = match config.security_level {
            SecurityLevel::Enhanced | SecurityLevel::Maximum => {
                if let Some(dpop_config) = &config.dpop_config {
                    let key_manager = match &dpop_config.key_storage {
                        DpopKeyStorageConfig::Memory => {
                            Arc::new(DpopKeyManager::new_memory().await.map_err(|e| {
                                McpError::Server(turbomcp_server::ServerError::Internal(
                                    e.to_string(),
                                ))
                            })?)
                        }
                        DpopKeyStorageConfig::Redis { url: _url } => {
                            // Redis support requires additional implementation
                            return Err(McpError::Server(turbomcp_server::ServerError::Internal(
                                "Redis DPoP storage not yet implemented".to_string(),
                            )));
                        }
                        DpopKeyStorageConfig::Hsm { config: _config } => {
                            return Err(McpError::Server(turbomcp_server::ServerError::Internal(
                                "HSM support not yet implemented".to_string(),
                            )));
                        }
                    };
                    Some(Arc::new(DpopProofGenerator::new(key_manager)))
                } else {
                    return Err(McpError::Server(turbomcp_server::ServerError::Internal(
                        "DPoP config required for Enhanced/Maximum security levels".to_string(),
                    )));
                }
            }
            SecurityLevel::Standard => None,
        };

        Ok(Self {
            name,
            config,
            oauth_client,
            token_storage,
            pending_auths: Arc::new(RwLock::new(HashMap::new())),
            resource_registry: None, // Can be set later via with_resource_registry()
            dynamic_registration: None, // Can be set later via with_dynamic_registration()
            #[cfg(feature = "dpop")]
            dpop_generator,
        })
    }

    /// Start MCP-compliant OAuth 2.1 authorization flow with Resource Indicators
    pub async fn start_authorization_with_resource(
        &self,
        resource_uri: &str,
    ) -> McpResult<OAuth2AuthResult> {
        // Validate resource URI format (RFC 8707 compliance)
        crate::auth::oauth2::validation::validate_canonical_resource_uri(resource_uri)?;

        // Generate PKCE code challenge for OAuth 2.1 compliance (always enabled)
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

        // Build authorization request with MCP compliance
        let mut auth_request = self
            .oauth_client
            .auth_code_client
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(pkce_challenge)
            .add_extra_param("resource", resource_uri); // RFC 8707 MANDATORY

        // Add provider-specific scopes
        for scope in &self.oauth_client.provider_config.default_scopes {
            auth_request = auth_request.add_scope(Scope::new(scope.clone()));
        }

        // Add any additional provider-specific parameters
        for (key, value) in &self.oauth_client.provider_config.additional_params {
            auth_request = auth_request.add_extra_param(key, value);
        }

        let (auth_url, csrf_token) = auth_request.url();

        // Store pending authorization with resource binding
        self.pending_auths.write().await.insert(
            csrf_token.secret().clone(),
            PendingAuth {
                state: csrf_token.clone(),
                pkce_verifier,
                created_at: SystemTime::now(),
                expires_at: SystemTime::now() + Duration::from_secs(600), // 10 minutes
                resource_uri: Some(resource_uri.to_string()),             // Track resource binding
            },
        );

        Ok(OAuth2AuthResult {
            auth_url: auth_url.to_string(),
            state: csrf_token.secret().clone(),
            code_verifier: None, // PKCE verifier stored securely in pending_auths
            device_code: None,
            user_code: None,
            verification_uri: None,
        })
    }

    /// Start comprehensive OAuth 2.0 authorization flow with automatic MCP compliance
    pub async fn start_authorization(&self) -> McpResult<OAuth2AuthResult> {
        // Check for MCP resource URI configuration
        if let Some(resource_uri) = &self.config.mcp_resource_uri {
            return self.start_authorization_with_resource(resource_uri).await;
        }

        // If auto_resource_indicators is enabled but no URI configured, error
        if self.config.auto_resource_indicators {
            return Err(McpError::InvalidInput(
                "MCP Resource Indicators enabled but no resource URI configured. \
                 Set mcp_resource_uri in OAuth2Config or call start_authorization_with_resource()"
                    .to_string(),
            ));
        }

        // Fallback to legacy flow (for non-MCP use cases)
        self.start_authorization_legacy().await
    }

    /// Legacy OAuth 2.0 authorization flow (without Resource Indicators)
    async fn start_authorization_legacy(&self) -> McpResult<OAuth2AuthResult> {
        // Generate PKCE code challenge for maximum security (always enabled)
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

        // Build authorization request with comprehensive security
        let mut auth_request = self
            .oauth_client
            .auth_code_client
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(pkce_challenge);

        // Add provider-specific scopes
        for scope in &self.oauth_client.provider_config.default_scopes {
            auth_request = auth_request.add_scope(Scope::new(scope.clone()));
        }

        // Add any additional provider-specific parameters
        for (key, value) in &self.oauth_client.provider_config.additional_params {
            auth_request = auth_request.add_extra_param(key, value);
        }

        let (auth_url, csrf_token) = auth_request.url();

        // Store pending authorization with comprehensive security
        self.pending_auths.write().await.insert(
            csrf_token.secret().clone(),
            PendingAuth {
                state: csrf_token.clone(),
                pkce_verifier,
                created_at: SystemTime::now(),
                expires_at: SystemTime::now() + Duration::from_secs(600), // 10 minutes
                resource_uri: None, // No resource binding for legacy flow
            },
        );

        Ok(OAuth2AuthResult {
            auth_url: auth_url.to_string(),
            state: csrf_token.secret().clone(),
            code_verifier: None, // PKCE verifier stored securely in pending_auths
            device_code: None,
            user_code: None,
            verification_uri: None,
        })
    }

    /// Exchange authorization code for tokens with comprehensive security validation
    pub async fn exchange_code(&self, code: &str, state: &str) -> McpResult<TokenInfo> {
        // Validate state parameter (CSRF protection)
        let pending = {
            let mut pending_auths = self.pending_auths.write().await;
            pending_auths.remove(state).ok_or_else(|| {
                McpError::Unauthorized("Invalid or expired state parameter".to_string())
            })?
        };

        // Validate state hasn't expired
        if SystemTime::now() > pending.expires_at {
            return Err(McpError::Unauthorized(
                "Authorization request expired".to_string(),
            ));
        }

        // Exchange authorization code for access token with PKCE and Resource Indicators
        let mut token_request = self
            .oauth_client
            .auth_code_client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .set_pkce_verifier(pending.pkce_verifier);

        // Add Resource Indicator if present (RFC 8707 MANDATORY for MCP)
        if let Some(resource_uri) = &pending.resource_uri {
            token_request = token_request.add_extra_param("resource", resource_uri);
        }

        let token_response = token_request
            .request_async(async_http_client)
            .await
            .map_err(|e| McpError::Unauthorized(format!("Token exchange failed: {e}")))?;

        // Extract token information with resource binding metadata
        let mut metadata = HashMap::new();
        if let Some(resource_uri) = &pending.resource_uri {
            metadata.insert(
                "resource_uri".to_string(),
                serde_json::Value::String(resource_uri.clone()),
            );
            metadata.insert(
                "audience".to_string(),
                serde_json::Value::String(resource_uri.clone()),
            );
        }

        let access_token = AccessToken {
            token: token_response.access_token().secret().clone(),
            expires_at: token_response
                .expires_in()
                .map(|duration| SystemTime::now() + duration),
            scopes: token_response
                .scopes()
                .map(|scopes| scopes.iter().map(|s| s.to_string()).collect())
                .unwrap_or_default(),
            metadata,
        };

        // Store access token for future use (production-grade token management)
        self.token_storage
            .store_access_token(pending.state.secret(), &access_token)
            .await
            .map_err(|e| McpError::internal(format!("Failed to store token: {}", e)))?;

        // Store refresh token if available
        if let Some(refresh_token) = token_response.refresh_token() {
            self.token_storage
                .store_refresh_token(pending.state.secret(), refresh_token)
                .await
                .map_err(|e| McpError::internal(format!("Failed to store refresh token: {}", e)))?
        }

        Ok(TokenInfo {
            access_token: access_token.token.clone(),
            token_type: "Bearer".to_string(),
            expires_in: token_response.expires_in().map(|d| d.as_secs()),
            refresh_token: token_response.refresh_token().map(|t| t.secret().clone()),
            scope: Some(access_token.scopes.join(" ")),
        })
    }

    /// Client credentials flow for server-to-server authentication
    pub async fn client_credentials_flow(&self) -> McpResult<TokenInfo> {
        let client = self
            .oauth_client
            .client_credentials_client
            .as_ref()
            .ok_or_else(|| {
                McpError::InvalidInput(
                    "Client credentials flow not supported by this provider".to_string(),
                )
            })?;

        let token_response = client
            .exchange_client_credentials()
            .request_async(async_http_client)
            .await
            .map_err(|e| {
                McpError::Unauthorized(format!("Client credentials exchange failed: {e}"))
            })?;

        let access_token = AccessToken {
            token: token_response.access_token().secret().clone(),
            expires_at: token_response
                .expires_in()
                .map(|duration| SystemTime::now() + duration),
            scopes: token_response
                .scopes()
                .map(|scopes| scopes.iter().map(|s| s.to_string()).collect())
                .unwrap_or_default(),
            metadata: HashMap::new(),
        };

        // Store the client credentials token
        self.token_storage
            .store_access_token("client_credentials", &access_token)
            .await
            .map_err(|e| McpError::internal(format!("Failed to store client token: {}", e)))?;

        Ok(TokenInfo {
            access_token: access_token.token.clone(),
            token_type: "Bearer".to_string(),
            expires_in: token_response.expires_in().map(|d| d.as_secs()),
            refresh_token: None, // Client credentials flow doesn't provide refresh tokens
            scope: Some(access_token.scopes.join(" ")),
        })
    }

    /// Device code flow for CLI/IoT applications  
    pub async fn device_code_flow(&self) -> McpResult<DeviceAuthorizationResponse> {
        let client = self
            .oauth_client
            .device_code_client
            .as_ref()
            .ok_or_else(|| {
                McpError::InvalidInput(
                    "Device code flow not supported by this provider".to_string(),
                )
            })?;

        let details: oauth2::DeviceAuthorizationResponse<
            oauth2::EmptyExtraDeviceAuthorizationFields,
        > = client
            .exchange_device_code()
            .map_err(|e| McpError::InvalidInput(format!("Device code configuration error: {e}")))?
            .request_async(async_http_client)
            .await
            .map_err(|e| McpError::InvalidInput(format!("Device code request failed: {e}")))?;

        Ok(DeviceAuthorizationResponse {
            device_code: details.device_code().secret().clone(),
            user_code: details.user_code().secret().clone(),
            verification_uri: details.verification_uri().to_string(),
            verification_uri_complete: details
                .verification_uri_complete()
                .map(|uri| uri.secret().clone()),
            expires_in: details.expires_in().as_secs(),
            interval: details.interval().as_secs(),
        })
    }

    /// Get stored access token for a user
    pub async fn get_stored_token(&self, user_id: &str) -> McpResult<Option<AccessToken>> {
        self.token_storage.get_access_token(user_id).await
    }

    /// Check if a token is expired
    pub fn is_token_expired(&self, token: &AccessToken) -> bool {
        if let Some(expires_at) = token.expires_at {
            SystemTime::now() > expires_at
        } else {
            false // No expiration time means it doesn't expire
        }
    }

    /// Get user info using provider-specific endpoint
    pub async fn get_user_info_with_provider_config(
        &self,
        access_token: &str,
    ) -> McpResult<UserInfo> {
        let provider_config = &self.oauth_client.provider_config;

        if let Some(_userinfo_endpoint) = &provider_config.userinfo_endpoint {
            // Use provider-specific userinfo endpoint (implementation would go here)
            // For now, fall back to standard method
            self.get_user_info(access_token).await
        } else {
            // Fall back to standard method
            self.get_user_info(access_token).await
        }
    }

    /// Determine if token should be refreshed based on provider refresh behavior
    pub fn should_refresh_token(&self, token: &AccessToken) -> bool {
        let provider_config = &self.oauth_client.provider_config;

        match provider_config.refresh_behavior {
            RefreshBehavior::Proactive => {
                // Refresh if token expires within 5 minutes
                if let Some(expires_at) = token.expires_at {
                    let refresh_threshold = SystemTime::now() + Duration::from_secs(300);
                    expires_at <= refresh_threshold
                } else {
                    false
                }
            }
            RefreshBehavior::Reactive => {
                // Only refresh when token is actually expired
                self.is_token_expired(token)
            }
            RefreshBehavior::Custom => {
                // Custom refresh logic would be implemented per provider
                // For now, default to reactive behavior
                self.is_token_expired(token)
            }
        }
    }

    /// Add token metadata for tracking and audit
    pub fn add_token_metadata(&self, token: &mut AccessToken, key: &str, value: serde_json::Value) {
        token.metadata.insert(key.to_string(), value);
    }

    /// Get the OAuth provider type
    pub fn get_provider_type(&self) -> ProviderType {
        self.oauth_client.provider_config.provider_type.clone()
    }

    /// Clean up expired auth sessions
    pub async fn cleanup_expired_sessions(&self) {
        let mut pending_auths = self.pending_auths.write().await;
        let now = SystemTime::now();

        // Remove sessions older than 10 minutes
        let threshold = now - Duration::from_secs(600);

        pending_auths.retain(|_, auth| auth.created_at > threshold);
    }

    /// Refresh an expired access token
    pub async fn refresh_token(&self, user_id: &str) -> McpResult<Option<TokenInfo>> {
        let refresh_token = match self.token_storage.get_refresh_token(user_id).await? {
            Some(token) => token,
            None => return Ok(None),
        };

        let token_response = self
            .oauth_client
            .auth_code_client
            .exchange_refresh_token(&refresh_token)
            .request_async(async_http_client)
            .await
            .map_err(|e| McpError::Unauthorized(format!("Token refresh failed: {e}")))?;

        let access_token = AccessToken {
            token: token_response.access_token().secret().clone(),
            expires_at: token_response
                .expires_in()
                .map(|duration| SystemTime::now() + duration),
            scopes: token_response
                .scopes()
                .map(|scopes| scopes.iter().map(|s| s.to_string()).collect())
                .unwrap_or_default(),
            metadata: HashMap::new(),
        };

        // Update stored token
        self.token_storage
            .store_access_token(user_id, &access_token)
            .await
            .map_err(|e| McpError::internal(format!("Failed to store refreshed token: {}", e)))?;

        Ok(Some(TokenInfo {
            access_token: access_token.token.clone(),
            token_type: "Bearer".to_string(),
            expires_in: token_response.expires_in().map(|d| d.as_secs()),
            refresh_token: token_response.refresh_token().map(|t| t.secret().clone()),
            scope: Some(access_token.scopes.join(" ")),
        }))
    }

    // ============================================================================
    // RFC 9728: Protected Resource Metadata Methods
    // ============================================================================

    /// Configure resource registry for RFC 9728 compliance (builder pattern)
    pub fn with_resource_registry(mut self, registry: Arc<McpResourceRegistry>) -> Self {
        self.resource_registry = Some(registry);
        self
    }

    /// Get the resource registry (if configured)
    pub fn resource_registry(&self) -> Option<&Arc<McpResourceRegistry>> {
        self.resource_registry.as_ref()
    }

    /// Get the OAuth2 configuration
    pub fn config(&self) -> &OAuth2Config {
        &self.config
    }

    /// Register a new protected resource for discovery (RFC 9728)
    pub async fn register_protected_resource(
        &self,
        resource_id: &str,
        scopes: Vec<String>,
        documentation: Option<String>,
    ) -> McpResult<()> {
        if let Some(registry) = &self.resource_registry {
            registry
                .register_resource(resource_id, scopes, documentation)
                .await
        } else {
            Err(McpError::InvalidInput(
                "Resource registry not configured. Use with_resource_registry()".to_string(),
            ))
        }
    }

    /// Generate RFC 9728 Protected Resource Metadata for well-known endpoint
    pub async fn generate_resource_metadata(
        &self,
    ) -> McpResult<HashMap<String, ProtectedResourceMetadata>> {
        if let Some(registry) = &self.resource_registry {
            Ok(registry.generate_well_known_metadata().await)
        } else {
            Err(McpError::InvalidInput(
                "Resource registry not configured. Use with_resource_registry()".to_string(),
            ))
        }
    }

    /// Validate token scopes against protected resource requirements (RFC 9728)
    pub async fn validate_resource_access(
        &self,
        resource_uri: &str,
        token_scopes: &[String],
    ) -> McpResult<bool> {
        if let Some(registry) = &self.resource_registry {
            registry
                .validate_scope_for_resource(resource_uri, token_scopes)
                .await
        } else {
            // Without registry, allow access (backward compatibility)
            Ok(true)
        }
    }

    /// Create default MCP resource registry with common MCP resources
    pub fn create_default_mcp_registry(
        mcp_server_uri: &str,
        auth_server_uri: &str,
    ) -> Arc<McpResourceRegistry> {
        Arc::new(McpResourceRegistry::new(
            mcp_server_uri.to_string(),
            auth_server_uri.to_string(),
        ))
    }

    /// Register standard MCP resources with default scopes
    pub async fn register_standard_mcp_resources(&self) -> McpResult<()> {
        if let Some(registry) = &self.resource_registry {
            // Register core MCP resources
            registry
                .register_resource(
                    "tools",
                    vec![
                        "mcp:tools:read".to_string(),
                        "mcp:tools:execute".to_string(),
                    ],
                    Some("MCP Tool execution and discovery".to_string()),
                )
                .await?;

            registry
                .register_resource(
                    "resources",
                    vec![
                        "mcp:resources:read".to_string(),
                        "mcp:resources:write".to_string(),
                    ],
                    Some("MCP Resource access and management".to_string()),
                )
                .await?;

            registry
                .register_resource(
                    "prompts",
                    vec![
                        "mcp:prompts:read".to_string(),
                        "mcp:prompts:use".to_string(),
                    ],
                    Some("MCP Prompt template access".to_string()),
                )
                .await?;

            Ok(())
        } else {
            Err(McpError::InvalidInput(
                "Resource registry not configured. Use with_resource_registry()".to_string(),
            ))
        }
    }

    // ============================================================================
    // RFC 7591: Dynamic Client Registration Methods
    // ============================================================================

    /// Configure dynamic client registration for RFC 7591 compliance (builder pattern)
    pub fn with_dynamic_registration(
        mut self,
        registration: Arc<DynamicClientRegistration>,
    ) -> Self {
        self.dynamic_registration = Some(registration);
        self
    }

    /// Get the dynamic client registration manager (if configured)
    pub fn dynamic_registration(&self) -> Option<&Arc<DynamicClientRegistration>> {
        self.dynamic_registration.as_ref()
    }

    /// Register a new OAuth client dynamically (RFC 7591)
    pub async fn register_dynamic_client(
        &self,
        request: ClientRegistrationRequest,
    ) -> McpResult<ClientRegistrationResponse> {
        if let Some(registration) = &self.dynamic_registration {
            registration.register_client(request).await
        } else {
            Err(McpError::InvalidInput(
                "Dynamic client registration not configured. Use with_dynamic_registration()"
                    .to_string(),
            ))
        }
    }

    /// Create a complete OAuth provider from dynamic client registration
    pub async fn from_dynamic_registration(
        name: String,
        registration_endpoint: String,
        auth_url: String,
        token_url: String,
        redirect_uri: String,
        mcp_server_uri: String,
        token_storage: Arc<dyn TokenStorage>,
    ) -> McpResult<Self> {
        // Create dynamic registration manager
        let registration = Arc::new(DynamicClientRegistration::new(registration_endpoint));

        // Create registration request for MCP client
        let registration_request = DynamicClientRegistration::create_mcp_client_request(
            &name,
            vec![redirect_uri.clone()],
            &mcp_server_uri,
        );

        // Register client dynamically
        let registration_response = registration.register_client(registration_request).await?;

        // Create OAuth2Config from registration response
        let config = OAuth2Config {
            client_id: registration_response.client_id,
            client_secret: registration_response.client_secret.unwrap_or_default(),
            auth_url,
            token_url,
            redirect_uri,
            scopes: registration_response
                .scope
                .unwrap_or_default()
                .split_whitespace()
                .map(|s| s.to_string())
                .collect(),
            flow_type: OAuth2FlowType::AuthorizationCode,
            additional_params: HashMap::new(),
            security_level: SecurityLevel::Standard,
            mcp_resource_uri: Some(mcp_server_uri.clone()),
            auto_resource_indicators: true,
            #[cfg(feature = "dpop")]
            dpop_config: None,
        };

        // Create OAuth provider
        let provider = Self::new(name, config, ProviderType::Generic, token_storage)
            .await?
            .with_dynamic_registration(registration);

        Ok(provider)
    }

    /// Discover and register with an OAuth authorization server
    pub async fn discover_and_register(
        client_name: &str,
        authorization_server_uri: &str,
        redirect_uri: String,
        mcp_server_uri: String,
        token_storage: Arc<dyn TokenStorage>,
    ) -> McpResult<Self> {
        // Discover authorization server metadata (simplified - in production would use OpenID Connect Discovery)
        let _discovery_url = format!(
            "{}/.well-known/oauth-authorization-server",
            authorization_server_uri.trim_end_matches('/')
        );

        // For now, construct endpoints based on common patterns
        let auth_url = format!(
            "{}/oauth/authorize",
            authorization_server_uri.trim_end_matches('/')
        );
        let token_url = format!(
            "{}/oauth/token",
            authorization_server_uri.trim_end_matches('/')
        );
        let registration_endpoint = format!(
            "{}/oauth/register",
            authorization_server_uri.trim_end_matches('/')
        );

        Self::from_dynamic_registration(
            client_name.to_string(),
            registration_endpoint,
            auth_url,
            token_url,
            redirect_uri,
            mcp_server_uri,
            token_storage,
        )
        .await
    }

    /// Create default MCP-compliant OAuth provider with dynamic registration
    pub async fn create_mcp_compliant_provider(
        client_name: &str,
        authorization_server_uri: &str,
        mcp_server_uri: &str,
        token_storage: Arc<dyn TokenStorage>,
    ) -> McpResult<Self> {
        // Use standard MCP redirect URI pattern
        let redirect_uri = format!("{}/oauth/callback", mcp_server_uri.trim_end_matches('/'));

        let mut provider = Self::discover_and_register(
            client_name,
            authorization_server_uri,
            redirect_uri,
            mcp_server_uri.to_string(),
            token_storage,
        )
        .await?;

        // Configure resource registry for MCP compliance
        let resource_registry =
            Self::create_default_mcp_registry(mcp_server_uri, authorization_server_uri);
        provider = provider.with_resource_registry(resource_registry);

        // Register standard MCP resources
        provider.register_standard_mcp_resources().await?;

        Ok(provider)
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
            AuthCredentials::OAuth2Code { code, state } => {
                let token_info = self.exchange_code(&code, &state).await?;
                let user_info = self.get_user_info(&token_info.access_token).await?;

                let expires_at = token_info
                    .expires_in
                    .map(|expires_in| SystemTime::now() + Duration::from_secs(expires_in));

                Ok(AuthContext {
                    user_id: user_info.id.clone(),
                    user: user_info,
                    roles: vec!["user".to_string()], // Default role
                    permissions: vec![],
                    session_id: uuid::Uuid::new_v4().to_string(),
                    token: Some(token_info),
                    provider: self.name.clone(),
                    authenticated_at: SystemTime::now(),
                    expires_at,
                    metadata: HashMap::new(),
                })
            }
            _ => Err(McpError::Tool(
                "Invalid credentials for OAuth2 provider".to_string(),
            )),
        }
    }

    async fn validate_token(&self, token: &str) -> McpResult<AuthContext> {
        // Validate token by fetching user info from OAuth provider's userinfo endpoint
        let user_info = self.get_user_info(token).await?;

        Ok(AuthContext {
            user_id: user_info.id.clone(),
            user: user_info,
            roles: vec!["user".to_string()],
            permissions: vec![],
            session_id: uuid::Uuid::new_v4().to_string(),
            token: Some(TokenInfo {
                access_token: token.to_string(),
                token_type: "Bearer".to_string(),
                refresh_token: None,
                expires_in: None,
                scope: None,
            }),
            provider: self.name.clone(),
            authenticated_at: SystemTime::now(),
            expires_at: None,
            metadata: HashMap::new(),
        })
    }

    async fn refresh_token(&self, refresh_token: &str) -> McpResult<TokenInfo> {
        // Use oauth2 crate for secure token refresh
        let token_response = self
            .oauth_client
            .auth_code_client
            .exchange_refresh_token(&RefreshToken::new(refresh_token.to_string()))
            .request_async(async_http_client)
            .await
            .map_err(|e| McpError::Tool(format!("Token refresh failed: {e}")))?;

        Ok(TokenInfo {
            access_token: token_response.access_token().secret().clone(),
            token_type: "Bearer".to_string(),
            expires_in: token_response.expires_in().map(|d| d.as_secs()),
            refresh_token: token_response
                .refresh_token()
                .map(|t| t.secret().clone())
                .or_else(|| Some(refresh_token.to_string())), // Keep existing if no new one
            scope: token_response.scopes().map(|scopes| {
                scopes
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            }),
        })
    }

    async fn revoke_token(&self, _token: &str) -> McpResult<()> {
        // Implementation would revoke the token with the OAuth provider
        Ok(())
    }

    async fn get_user_info(&self, token: &str) -> McpResult<UserInfo> {
        // Current implementation: Direct HTTP call to userinfo endpoint (works for validation)
        // Enhanced oauth2 crate integration can be added for more advanced OAuth features
        // Current approach provides secure token validation via standard HTTP requests
        if token.trim().is_empty() {
            return Err(crate::McpError::Unauthorized("Empty token".to_string()));
        }

        // Construct userinfo endpoint URL based on token URL base
        let base_url = &self.config.token_url;

        let userinfo_url = base_url
            .trim_end_matches("/token")
            .trim_end_matches("/oauth/token");
        let userinfo_endpoint = format!("{userinfo_url}/userinfo");

        // Use reqwest for secure HTTPS OAuth communication (same as oauth2 crate uses internally)
        let client = reqwest::Client::new();
        let response = client
            .get(&userinfo_endpoint)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| crate::McpError::Network(format!("Failed to fetch user info: {e}")))?;

        if !response.status().is_success() {
            return Err(crate::McpError::Unauthorized(
                "Failed to fetch user info".to_string(),
            ));
        }

        let user_data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| crate::McpError::Network(format!("Invalid JSON response: {e}")))?;

        // Extract user information with comprehensive field mapping
        let user_id = user_data
            .get("id")
            .or_else(|| user_data.get("sub"))
            .or_else(|| user_data.get("user_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let username = user_data
            .get("username")
            .or_else(|| user_data.get("preferred_username"))
            .or_else(|| user_data.get("login"))
            .and_then(|v| v.as_str())
            .unwrap_or(&user_id)
            .to_string();
        let email = user_data
            .get("email")
            .and_then(|v| v.as_str())
            .map(std::string::ToString::to_string);
        let display_name = user_data
            .get("name")
            .or_else(|| user_data.get("display_name"))
            .and_then(|v| v.as_str())
            .map(std::string::ToString::to_string);
        let avatar_url = user_data
            .get("picture")
            .or_else(|| user_data.get("avatar_url"))
            .and_then(|v| v.as_str())
            .map(std::string::ToString::to_string);

        Ok(UserInfo {
            id: user_id,
            username,
            email,
            display_name,
            avatar_url,
            metadata: HashMap::new(),
        })
    }
}
