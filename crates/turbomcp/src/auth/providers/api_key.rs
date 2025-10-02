//! API Key Authentication Provider
//!
//! Simple API key-based authentication for service-to-service communication.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use tokio::sync::RwLock;

use super::super::config::AuthProviderType;
use super::super::types::{AuthContext, AuthCredentials, AuthProvider, TokenInfo, UserInfo};
use crate::{McpError, McpResult};

/// API Key authentication provider
#[derive(Debug)]
pub struct ApiKeyProvider {
    /// Provider name
    name: String,
    /// Valid API keys with associated user info
    api_keys: Arc<RwLock<HashMap<String, UserInfo>>>,
}

impl ApiKeyProvider {
    /// Create a new API key provider
    #[must_use]
    pub fn new(name: String) -> Self {
        Self {
            name,
            api_keys: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add an API key
    pub async fn add_api_key(&self, key: String, user_info: UserInfo) {
        self.api_keys.write().await.insert(key, user_info);
    }

    /// Remove an API key
    pub async fn remove_api_key(&self, key: &str) -> bool {
        self.api_keys.write().await.remove(key).is_some()
    }

    /// List all API keys (returns keys only, not full info for security)
    pub async fn list_api_keys(&self) -> Vec<String> {
        self.api_keys.read().await.keys().cloned().collect()
    }
}

#[async_trait]
impl AuthProvider for ApiKeyProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn provider_type(&self) -> AuthProviderType {
        AuthProviderType::ApiKey
    }

    async fn authenticate(&self, credentials: AuthCredentials) -> McpResult<AuthContext> {
        match credentials {
            AuthCredentials::ApiKey { key } => {
                let api_keys = self.api_keys.read().await;
                if let Some(user_info) = api_keys.get(&key) {
                    Ok(AuthContext {
                        user_id: user_info.id.clone(),
                        user: user_info.clone(),
                        roles: vec!["api_user".to_string()],
                        permissions: vec!["api_access".to_string()],
                        session_id: uuid::Uuid::new_v4().to_string(),
                        token: Some(TokenInfo {
                            access_token: key,
                            token_type: "ApiKey".to_string(),
                            refresh_token: None,
                            expires_in: None,
                            scope: None,
                        }),
                        provider: self.name.clone(),
                        authenticated_at: SystemTime::now(),
                        expires_at: None,
                        metadata: HashMap::new(),
                    })
                } else {
                    Err(McpError::Tool("Invalid API key".to_string()))
                }
            }
            _ => Err(McpError::Tool(
                "Invalid credentials for API key provider".to_string(),
            )),
        }
    }

    async fn validate_token(&self, token: &str) -> McpResult<AuthContext> {
        self.authenticate(AuthCredentials::ApiKey {
            key: token.to_string(),
        })
        .await
    }

    async fn refresh_token(&self, _refresh_token: &str) -> McpResult<TokenInfo> {
        Err(McpError::Tool(
            "API keys do not support token refresh".to_string(),
        ))
    }

    async fn revoke_token(&self, token: &str) -> McpResult<()> {
        let removed = self.remove_api_key(token).await;
        if removed {
            Ok(())
        } else {
            Err(McpError::Tool("API key not found".to_string()))
        }
    }

    async fn get_user_info(&self, token: &str) -> McpResult<UserInfo> {
        let api_keys = self.api_keys.read().await;
        api_keys
            .get(token)
            .cloned()
            .ok_or_else(|| McpError::Tool("Invalid API key".to_string()))
    }
}
