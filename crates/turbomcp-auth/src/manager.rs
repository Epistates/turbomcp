//! Authentication Manager
//!
//! Central authentication manager for coordinating multiple authentication providers.
//!
//! # MCP Compliance
//!
//! Per MCP specification (2025-06-18), authentication is **stateless**.
//! Each request must include valid credentials (Bearer token in Authorization header).
//! This manager does NOT maintain server-side session state for authentication decisions.
//!
//! ## Stateless Authentication Flow
//!
//! ```rust,no_run
//! # use turbomcp_auth::{AuthManager, AuthCredentials, config::AuthConfig};
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! # let config = AuthConfig {
//! #     enabled: true,
//! #     providers: vec![],
//! #     authorization: Default::default(),
//! # };
//! # let manager = AuthManager::new(config);
//! # let credentials = AuthCredentials::ApiKey { key: "test".to_string() };
//! // 1. Authenticate user and get auth context
//! let auth_context = manager.authenticate("oauth2", credentials).await?;
//!
//! // 2. Extract token from auth context
//! let token = auth_context.token.as_ref().unwrap().access_token.clone();
//!
//! // 3. On subsequent requests, validate token EVERY TIME
//! let validated_context = manager.validate_token(&token, Some("oauth2")).await?;
//! // ✅ Token validated via provider - truly stateless
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use super::config::AuthConfig;
use super::context::AuthContext as UnifiedAuthContext; // Unified AuthContext for external API
use super::types::{AuthCredentials, AuthProvider};
use turbomcp_protocol::{Error as McpError, Result as McpResult};

/// Authentication manager for coordinating multiple authentication providers
///
/// # MCP Specification Compliance
///
/// This manager implements **stateless** authentication per MCP spec (RFC 9728).
/// No server-side session state is maintained. All authentication decisions are made
/// by validating credentials on EVERY request.
#[derive(Debug)]
pub struct AuthManager {
    /// Authentication configuration
    config: AuthConfig,
    /// Registered authentication providers
    providers: Arc<RwLock<HashMap<String, Arc<dyn AuthProvider>>>>,
}

impl AuthManager {
    /// Create a new authentication manager
    ///
    /// # MCP Specification Compliance
    ///
    /// Creates a stateless authentication manager per MCP spec.
    /// No server-side session state is maintained.
    #[must_use]
    pub fn new(config: AuthConfig) -> Self {
        Self {
            config,
            providers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add an authentication provider
    pub async fn add_provider(&self, provider: Arc<dyn AuthProvider>) {
        let name = provider.name().to_string();
        self.providers.write().await.insert(name, provider);
    }

    /// Remove an authentication provider
    pub async fn remove_provider(&self, name: &str) -> bool {
        self.providers.write().await.remove(name).is_some()
    }

    /// List available providers
    pub async fn list_providers(&self) -> Vec<String> {
        self.providers.read().await.keys().cloned().collect()
    }

    /// Authenticate user with credentials
    ///
    /// # MCP Specification Compliance
    ///
    /// Authenticates the user and returns an `AuthContext`.
    /// **NO server-side session state is created** - per MCP stateless requirement.
    ///
    /// The returned `AuthContext` contains a token (if applicable) that the client
    /// must include in subsequent requests via the `Authorization` header.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use turbomcp_auth::{AuthManager, AuthCredentials, config::AuthConfig};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let config = AuthConfig {
    /// #     enabled: true,
    /// #     providers: vec![],
    /// #     authorization: Default::default(),
    /// # };
    /// # let manager = AuthManager::new(config);
    /// let credentials = AuthCredentials::ApiKey {
    ///     key: "secret_key".to_string(),
    /// };
    ///
    /// let auth_context = manager.authenticate("api", credentials).await?;
    ///
    /// // Extract token for subsequent requests
    /// if let Some(token_info) = &auth_context.token {
    ///     let access_token = &token_info.access_token;
    ///     // Client must send: Authorization: Bearer {access_token}
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn authenticate(
        &self,
        provider_name: &str,
        credentials: AuthCredentials,
    ) -> McpResult<UnifiedAuthContext> {
        if !self.config.enabled {
            return Err(McpError::internal("Authentication is disabled".to_string()));
        }

        let providers = self.providers.read().await;
        let provider = providers
            .get(provider_name)
            .ok_or_else(|| McpError::internal(format!("Provider '{provider_name}' not found")))?;

        let mut auth_context = provider.authenticate(credentials).await?;

        // Apply default roles if configured
        if auth_context.roles.is_empty() {
            auth_context.roles = self.config.authorization.default_roles.clone();
        }

        // MCP Spec: Stateless authentication - NO session storage
        // Client must include token in Authorization header on every request
        Ok(auth_context)
    }

    /// Validate token and get authentication context
    ///
    /// # MCP Specification Compliance
    ///
    /// Validates the token on EVERY request per MCP stateless requirement.
    /// This method MUST be called for each incoming request to ensure the token
    /// is still valid (not expired, not revoked, etc.).
    ///
    /// # Arguments
    ///
    /// * `token` - The access token to validate (from Authorization header)
    /// * `provider_name` - Optional provider name (if known). If None, tries all providers.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use turbomcp_auth::AuthManager;
    /// # async fn handle_request(manager: &AuthManager, auth_header: &str) -> Result<(), Box<dyn std::error::Error>> {
    /// // Extract token from Authorization header
    /// let token = auth_header.strip_prefix("Bearer ").unwrap();
    ///
    /// // Validate token on EVERY request (stateless)
    /// let auth_context = manager.validate_token(token, None).await?;
    ///
    /// // Use auth_context for authorization decisions
    /// println!("Authenticated user: {}", auth_context.user.username);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn validate_token(
        &self,
        token: &str,
        provider_name: Option<&str>,
    ) -> McpResult<UnifiedAuthContext> {
        if !self.config.enabled {
            return Err(McpError::internal("Authentication is disabled".to_string()));
        }

        let providers = self.providers.read().await;

        if let Some(provider_name) = provider_name {
            let provider = providers.get(provider_name).ok_or_else(|| {
                McpError::internal(format!("Provider '{provider_name}' not found"))
            })?;
            provider.validate_token(token).await
        } else {
            // Try all providers
            for provider in providers.values() {
                if let Ok(auth_context) = provider.validate_token(token).await {
                    return Ok(auth_context);
                }
            }
            Err(McpError::internal("Token validation failed".to_string()))
        }
    }

    /// Check if user has permission
    #[must_use]
    pub fn check_permission(&self, context: &UnifiedAuthContext, permission: &str) -> bool {
        context.permissions.contains(&permission.to_string())
            || context.roles.iter().any(|role| {
                self.config
                    .authorization
                    .inheritance_rules
                    .get(role)
                    .is_some_and(|perms| perms.contains(&permission.to_string()))
            })
    }

    /// Check if user has role
    #[must_use]
    pub fn check_role(&self, context: &UnifiedAuthContext, role: &str) -> bool {
        context.roles.contains(&role.to_string())
    }
}

// Note: PKCE functionality is handled by the oauth2 crate's built-in
// PkceCodeChallenge::new_random_sha256() method for maximum security

/// Global authentication manager
static GLOBAL_AUTH_MANAGER: once_cell::sync::Lazy<tokio::sync::RwLock<Option<Arc<AuthManager>>>> =
    once_cell::sync::Lazy::new(|| tokio::sync::RwLock::new(None));

/// Set the global authentication manager
pub async fn set_global_auth_manager(manager: Arc<AuthManager>) {
    *GLOBAL_AUTH_MANAGER.write().await = Some(manager);
}

/// Get the global authentication manager
pub async fn global_auth_manager() -> Option<Arc<AuthManager>> {
    GLOBAL_AUTH_MANAGER.read().await.clone()
}

/// Convenience function to check authentication
pub async fn check_auth(token: &str) -> McpResult<UnifiedAuthContext> {
    if let Some(manager) = global_auth_manager().await {
        manager.validate_token(token, None).await
    } else {
        Err(McpError::internal(
            "Authentication manager not initialized".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{AuthorizationConfig, OAuth2Config, OAuth2FlowType, SecurityLevel},
        providers::ApiKeyProvider,
        types::UserInfo,
    };
    use std::collections::HashMap;

    #[test]
    fn test_oauth2_config() {
        let config = OAuth2Config {
            client_id: "test_client".to_string(),
            client_secret: "test_secret".to_string().into(),
            auth_url: "https://auth.example.com/oauth/authorize".to_string(),
            token_url: "https://auth.example.com/oauth/token".to_string(),
            revocation_url: None,
            redirect_uri: "http://localhost:8080/callback".to_string(),
            scopes: vec!["read".to_string(), "write".to_string()],
            flow_type: OAuth2FlowType::AuthorizationCode,
            additional_params: HashMap::new(),
            security_level: SecurityLevel::Standard,
            mcp_resource_uri: None,
            auto_resource_indicators: false,
            #[cfg(feature = "dpop")]
            dpop_config: None,
        };

        assert_eq!(config.client_id, "test_client");
        assert_eq!(config.flow_type, OAuth2FlowType::AuthorizationCode);
    }

    #[test]
    fn test_oauth2_pkce_integration() {
        // Test that oauth2 crate PKCE functionality works as expected
        let (challenge1, _verifier1) = oauth2::PkceCodeChallenge::new_random_sha256();
        let (challenge2, _verifier2) = oauth2::PkceCodeChallenge::new_random_sha256();

        // Each PKCE challenge should be unique
        assert_ne!(challenge1.as_str(), challenge2.as_str());
        assert!(!challenge1.as_str().is_empty());
        assert!(!challenge2.as_str().is_empty());
    }

    #[tokio::test]
    async fn test_api_key_provider() {
        let provider = ApiKeyProvider::new("test_api".to_string());

        let user_info = UserInfo {
            id: "user123".to_string(),
            username: "testuser".to_string(),
            email: Some("test@example.com".to_string()),
            display_name: Some("Test User".to_string()),
            avatar_url: None,
            metadata: HashMap::new(),
        };

        provider
            .add_api_key("test_key_123".to_string(), user_info.clone())
            .await;

        let credentials = AuthCredentials::ApiKey {
            key: "test_key_123".to_string(),
        };

        let auth_result = provider.authenticate(credentials).await;
        assert!(auth_result.is_ok());

        let context = auth_result.unwrap();
        assert_eq!(context.user.username, "testuser");
        assert_eq!(context.provider, "test_api");
    }

    #[tokio::test]
    async fn test_auth_manager() {
        let config = AuthConfig {
            enabled: true,
            providers: vec![],
            authorization: AuthorizationConfig {
                rbac_enabled: true,
                default_roles: vec!["user".to_string()],
                inheritance_rules: HashMap::new(),
                resource_permissions: HashMap::new(),
            },
        };

        let manager = AuthManager::new(config);
        let api_provider = Arc::new(ApiKeyProvider::new("api".to_string()));
        manager.add_provider(api_provider.clone()).await;

        let providers = manager.list_providers().await;
        assert!(providers.contains(&"api".to_string()));
    }
}
