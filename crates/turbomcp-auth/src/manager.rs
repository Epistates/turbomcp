//! Authentication Manager
//!
//! Central authentication manager for coordinating multiple authentication providers.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio::sync::RwLock;

use super::config::AuthConfig;
use super::types::{AuthContext, AuthCredentials, AuthProvider};
use turbomcp_core::{Error as McpError, Result as McpResult};

/// Authentication manager for coordinating multiple authentication providers
#[derive(Debug)]
pub struct AuthManager {
    /// Authentication configuration
    config: AuthConfig,
    /// Registered authentication providers
    providers: Arc<RwLock<HashMap<String, Arc<dyn AuthProvider>>>>,
    /// Active sessions
    sessions: Arc<RwLock<HashMap<String, AuthContext>>>,
    /// Session cleanup task handle
    _cleanup_handle: Option<tokio::task::JoinHandle<()>>,
}

impl AuthManager {
    /// Create a new authentication manager
    #[must_use]
    pub fn new(config: AuthConfig) -> Self {
        let manager = Self {
            config,
            providers: Arc::new(RwLock::new(HashMap::new())),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            _cleanup_handle: None,
        };

        // Start session cleanup task
        let sessions_clone = manager.sessions.clone();
        let cleanup_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(300)); // 5 minutes
            loop {
                interval.tick().await;
                let now = SystemTime::now();
                let mut sessions = sessions_clone.write().await;
                sessions
                    .retain(|_, context| context.expires_at.is_none_or(|expires| expires > now));
            }
        });

        Self {
            _cleanup_handle: Some(cleanup_handle),
            ..manager
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
    pub async fn authenticate(
        &self,
        provider_name: &str,
        credentials: AuthCredentials,
    ) -> McpResult<AuthContext> {
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

        // Store session
        let session_id = auth_context.session_id.clone();
        self.sessions
            .write()
            .await
            .insert(session_id, auth_context.clone());

        Ok(auth_context)
    }

    /// Validate token and get authentication context
    pub async fn validate_token(
        &self,
        token: &str,
        provider_name: Option<&str>,
    ) -> McpResult<AuthContext> {
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
                if let Ok(context) = provider.validate_token(token).await {
                    return Ok(context);
                }
            }
            Err(McpError::internal("Token validation failed".to_string()))
        }
    }

    /// Get session by ID
    pub async fn get_session(&self, session_id: &str) -> Option<AuthContext> {
        self.sessions.read().await.get(session_id).cloned()
    }

    /// Revoke session
    pub async fn revoke_session(&self, session_id: &str) -> McpResult<()> {
        let context = self
            .sessions
            .write()
            .await
            .remove(session_id)
            .ok_or_else(|| McpError::internal("Session not found".to_string()))?;

        // Try to revoke token with provider
        let providers = self.providers.read().await;
        if let Some(provider) = providers.get(&context.provider)
            && let Some(token) = &context.token
        {
            let _ = provider.revoke_token(&token.access_token).await;
        }

        Ok(())
    }

    /// Check if user has permission
    #[must_use]
    pub fn check_permission(&self, context: &AuthContext, permission: &str) -> bool {
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
    pub fn check_role(&self, context: &AuthContext, role: &str) -> bool {
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
pub async fn check_auth(token: &str) -> McpResult<AuthContext> {
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
        config::{
            AuthorizationConfig, OAuth2Config, OAuth2FlowType, SecurityLevel, SessionConfig,
            SessionStorageType,
        },
        providers::ApiKeyProvider,
        types::UserInfo,
    };
    use std::collections::HashMap;

    #[test]
    fn test_oauth2_config() {
        let config = OAuth2Config {
            client_id: "test_client".to_string(),
            client_secret: "test_secret".to_string(),
            auth_url: "https://auth.example.com/oauth/authorize".to_string(),
            token_url: "https://auth.example.com/oauth/token".to_string(),
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
            session: SessionConfig {
                timeout_seconds: 3600,
                secure_cookies: true,
                cookie_domain: None,
                storage: SessionStorageType::Memory,
                max_sessions_per_user: Some(5),
            },
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
