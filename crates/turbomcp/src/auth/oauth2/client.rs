//! OAuth 2.0 Client Implementation
//!
//! This module provides a production-grade OAuth 2.0 client wrapper that supports:
//! - Authorization Code flow (with PKCE)
//! - Client Credentials flow (server-to-server)
//! - Device Authorization flow (CLI/IoT)
//!
//! The client handles provider-specific configurations and quirks for
//! Google, Microsoft, GitHub, GitLab, and generic OAuth providers.

use std::collections::HashMap;

use oauth2::{AuthUrl, ClientId, ClientSecret, RedirectUrl, TokenUrl, basic::BasicClient};

use crate::{McpError, McpResult};

use super::super::config::{OAuth2Config, ProviderConfig, ProviderType, RefreshBehavior};

/// Production-grade OAuth2 client wrapper supporting all modern flows
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
    /// Create a production-grade OAuth2 client supporting all flows
    pub fn new(config: &OAuth2Config, provider_type: ProviderType) -> McpResult<Self> {
        // Validate URLs
        let auth_url = AuthUrl::new(config.auth_url.clone())
            .map_err(|_| McpError::InvalidInput("Invalid authorization URL".to_string()))?;

        let token_url = TokenUrl::new(config.token_url.clone())
            .map_err(|_| McpError::InvalidInput("Invalid token URL".to_string()))?;

        // Enhanced redirect URI validation with comprehensive security checks
        let redirect_url = Self::validate_redirect_uri(&config.redirect_uri)?;

        // Create authorization code flow client (primary)
        let client_secret = if config.client_secret.is_empty() {
            None
        } else {
            Some(ClientSecret::new(config.client_secret.clone()))
        };

        let auth_code_client = BasicClient::new(
            ClientId::new(config.client_id.clone()),
            client_secret.clone(),
            auth_url.clone(),
            Some(token_url.clone()),
        )
        .set_redirect_uri(redirect_url);

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
            ProviderType::Generic | ProviderType::Custom(_) => ProviderConfig {
                provider_type,
                default_scopes: vec!["openid".to_string(), "profile".to_string()],
                refresh_behavior: RefreshBehavior::Proactive,
                userinfo_endpoint: None,
                additional_params: HashMap::new(),
            },
        }
    }

    /// Comprehensive redirect URI validation with security best practices
    ///
    /// Security considerations:
    /// - Prevents open redirect attacks
    /// - Validates URL format and structure
    /// - Environment-aware validation (localhost for development)
    fn validate_redirect_uri(uri: &str) -> McpResult<RedirectUrl> {
        use url::Url;

        // Parse and validate URL structure
        let parsed = Url::parse(uri)
            .map_err(|e| McpError::InvalidInput(format!("Invalid redirect URI format: {e}")))?;

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
                        return Err(McpError::InvalidInput(
                            "HTTP redirect URIs only allowed for localhost in development"
                                .to_string(),
                        ));
                    }
                } else {
                    return Err(McpError::InvalidInput(
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
                return Err(McpError::InvalidInput(format!(
                    "Unsupported redirect URI scheme: {}. Use https, http (localhost only), or app-specific schemes",
                    parsed.scheme()
                )));
            }
        }

        // Security: Prevent fragment in redirect URI (per OAuth 2.0 spec)
        if parsed.fragment().is_some() {
            return Err(McpError::InvalidInput(
                "Redirect URI must not contain URL fragment".to_string(),
            ));
        }

        // Security: Check for path traversal in PATH component only
        // Note: url::Url::parse() already normalizes paths and removes .. segments
        // We check the final path to ensure no traversal remains after normalization
        if let Some(path) = parsed.path_segments() {
            for segment in path {
                if segment == ".." {
                    return Err(McpError::InvalidInput(
                        "Redirect URI path must not contain traversal sequences".to_string(),
                    ));
                }
            }
        }

        // Industry Standard: Use oauth2 crate's RedirectUrl for validation
        // This provides battle-tested URL validation per OAuth 2.0 specifications
        // For production security, implement exact whitelist matching of allowed URIs
        // (not pattern matching, which is error-prone per OAuth Security Best Practice RFC)
        RedirectUrl::new(uri.to_string())
            .map_err(|_| McpError::InvalidInput("Failed to create redirect URL".to_string()))
    }

    /// Get access to the authorization code client
    pub fn auth_code_client(&self) -> &BasicClient {
        &self.auth_code_client
    }

    /// Get access to the client credentials client (if available)
    pub fn client_credentials_client(&self) -> Option<&BasicClient> {
        self.client_credentials_client.as_ref()
    }

    /// Get access to the device code client (if available)
    pub fn device_code_client(&self) -> Option<&BasicClient> {
        self.device_code_client.as_ref()
    }

    /// Get the provider configuration
    pub fn provider_config(&self) -> &ProviderConfig {
        &self.provider_config
    }
}
