//! API Key Authentication Provider
//!
//! Simple API key-based authentication for service-to-service communication.
//!
//! ## Security
//!
//! Plaintext API keys never appear in this provider's storage. On insert, the key is
//! hashed with BLAKE3 (cryptographic, fast, fixed 32-byte digest); only the digest is
//! retained. On validation, the input is hashed and the digest is looked up — same
//! constant-time hash on every input regardless of correctness, no plaintext-comparison
//! oracle, and no plaintext to leak via panic/serialization/memory inspection.
//!
//! See [`crate::api_key_validation`] for the underlying primitives.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::RwLock;

use super::super::api_key_validation::MIN_API_KEY_LENGTH;
use super::super::config::AuthProviderType;
use super::super::context::AuthContext;
use super::super::types::{AuthCredentials, AuthProvider, TokenInfo, UserInfo};
use turbomcp_protocol::{Error as McpError, Result as McpResult};

/// 32-byte BLAKE3 digest of an API key. Used as the at-rest storage form.
type KeyHash = [u8; 32];

#[inline]
fn hash_api_key(key: &str) -> KeyHash {
    blake3::hash(key.as_bytes()).into()
}

/// API Key authentication provider
#[derive(Debug)]
pub struct ApiKeyProvider {
    /// Provider name
    name: String,
    /// BLAKE3-hashed API keys → user info. Plaintext keys are never stored here.
    api_keys: Arc<RwLock<HashMap<KeyHash, UserInfo>>>,
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

    /// Add an API key. The plaintext is hashed with BLAKE3 before storage; the original
    /// `key` value passed in is dropped at the end of this call and is never retained.
    ///
    /// Returns an error if the key is shorter than [`MIN_API_KEY_LENGTH`].
    pub async fn add_api_key(&self, key: String, user_info: UserInfo) -> McpResult<()> {
        if key.len() < MIN_API_KEY_LENGTH {
            return Err(McpError::invalid_params(format!(
                "API key must be at least {MIN_API_KEY_LENGTH} characters"
            )));
        }
        let hash = hash_api_key(&key);
        self.api_keys.write().await.insert(hash, user_info);
        Ok(())
    }

    /// Remove an API key by its plaintext value (the key is hashed internally).
    pub async fn remove_api_key(&self, key: &str) -> bool {
        let hash = hash_api_key(key);
        self.api_keys.write().await.remove(&hash).is_some()
    }

    /// Return how many API keys are stored. Plaintext key listing is intentionally
    /// not exposed — once hashed, the originals cannot be recovered.
    pub async fn api_key_count(&self) -> usize {
        self.api_keys.read().await.len()
    }
}

impl AuthProvider for ApiKeyProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn provider_type(&self) -> AuthProviderType {
        AuthProviderType::ApiKey
    }

    fn authenticate(
        &self,
        credentials: AuthCredentials,
    ) -> Pin<Box<dyn Future<Output = McpResult<AuthContext>> + Send + '_>> {
        Box::pin(async move {
            match credentials {
                AuthCredentials::ApiKey { key } => {
                    if key.len() < MIN_API_KEY_LENGTH {
                        return Err(McpError::internal("Invalid API key".to_string()));
                    }
                    // Hash the input once (constant-time on input length) and look up the
                    // digest. Plaintext keys are never stored, so neither side of the
                    // comparison sees them.
                    let hash = hash_api_key(&key);
                    let user_info = {
                        let api_keys = self.api_keys.read().await;
                        api_keys.get(&hash).cloned()
                    };

                    if let Some(user_info) = user_info {
                        let token = TokenInfo {
                            access_token: key,
                            token_type: "ApiKey".to_string(),
                            refresh_token: None,
                            expires_in: None,
                            issued_at: Some(std::time::SystemTime::now()),
                            scope: None,
                        };

                        AuthContext::builder()
                            .subject(user_info.id.clone())
                            .user(user_info.clone())
                            .roles(vec!["api_user".to_string()])
                            .permissions(vec!["api_access".to_string()])
                            .request_id(uuid::Uuid::new_v4().to_string())
                            .token(token)
                            .provider(self.name.clone())
                            .build()
                            .map_err(|e| McpError::internal(e.to_string()))
                    } else {
                        Err(McpError::internal("Invalid API key".to_string()))
                    }
                }
                _ => Err(McpError::internal(
                    "Invalid credentials for API key provider".to_string(),
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
            self.authenticate(AuthCredentials::ApiKey { key: token })
                .await
        })
    }

    fn refresh_token(
        &self,
        _refresh_token: &str,
    ) -> Pin<Box<dyn Future<Output = McpResult<TokenInfo>> + Send + '_>> {
        Box::pin(async {
            Err(McpError::internal(
                "API keys do not support token refresh".to_string(),
            ))
        })
    }

    fn revoke_token(
        &self,
        token: &str,
    ) -> Pin<Box<dyn Future<Output = McpResult<()>> + Send + '_>> {
        let token = token.to_string();
        Box::pin(async move {
            let removed = self.remove_api_key(&token).await;
            if removed {
                Ok(())
            } else {
                Err(McpError::internal("API key not found".to_string()))
            }
        })
    }

    fn get_user_info(
        &self,
        token: &str,
    ) -> Pin<Box<dyn Future<Output = McpResult<UserInfo>> + Send + '_>> {
        let token = token.to_string();
        Box::pin(async move {
            if token.len() < MIN_API_KEY_LENGTH {
                return Err(McpError::internal("Invalid API key".to_string()));
            }
            let hash = hash_api_key(&token);
            let api_keys = self.api_keys.read().await;
            api_keys
                .get(&hash)
                .cloned()
                .ok_or_else(|| McpError::internal("Invalid API key".to_string()))
        })
    }
}
