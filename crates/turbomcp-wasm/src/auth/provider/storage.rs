//! Storage traits for OAuth 2.1 provider.
//!
//! This module provides pluggable storage backends for OAuth state:
//! - Authorization codes
//! - Access tokens
//! - Refresh tokens
//!
//! # Security
//!
//! Tokens are stored by hash only (never plaintext) and grant data is encrypted
//! using the token as key material. This follows the security patterns from
//! Cloudflare's workers-oauth-provider.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

/// Result type for storage operations.
pub type StorageResult<T> = Result<T, StorageError>;

/// Errors that can occur during storage operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageError {
    /// Item not found
    NotFound(String),
    /// Item has expired
    Expired(String),
    /// Storage backend error
    Backend(String),
    /// Serialization error
    Serialization(String),
    /// Encryption/decryption error
    Crypto(String),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(key) => write!(f, "Not found: {}", key),
            Self::Expired(key) => write!(f, "Expired: {}", key),
            Self::Backend(msg) => write!(f, "Storage error: {}", msg),
            Self::Serialization(msg) => write!(f, "Serialization error: {}", msg),
            Self::Crypto(msg) => write!(f, "Crypto error: {}", msg),
        }
    }
}

impl std::error::Error for StorageError {}

/// Authorization code grant data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizationCodeGrant {
    /// Client that requested authorization
    pub client_id: String,
    /// Redirect URI used in the request
    pub redirect_uri: String,
    /// Scopes granted
    pub scopes: Vec<String>,
    /// PKCE code challenge
    pub code_challenge: Option<String>,
    /// PKCE code challenge method (S256 or plain)
    pub code_challenge_method: Option<String>,
    /// User/subject identifier
    pub subject: String,
    /// Expiration timestamp (Unix seconds)
    pub expires_at: u64,
    /// Nonce for OpenID Connect
    pub nonce: Option<String>,
    /// State parameter
    pub state: Option<String>,
}

/// Access token data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessTokenData {
    /// Subject (user) the token was issued for
    pub subject: String,
    /// Client the token was issued to
    pub client_id: String,
    /// Scopes granted
    pub scopes: Vec<String>,
    /// Expiration timestamp (Unix seconds)
    pub expires_at: u64,
    /// Issue timestamp (Unix seconds)
    pub issued_at: u64,
    /// Associated refresh token hash (for revocation)
    pub refresh_token_hash: Option<String>,
}

/// Refresh token data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshTokenData {
    /// Subject (user) the token was issued for
    pub subject: String,
    /// Client the token was issued to
    pub client_id: String,
    /// Scopes granted
    pub scopes: Vec<String>,
    /// Expiration timestamp (Unix seconds)
    pub expires_at: u64,
    /// Issue timestamp (Unix seconds)
    pub issued_at: u64,
    /// Generation number (incremented on refresh for dual-token resilience)
    pub generation: u32,
    /// Family ID (links all tokens in a refresh chain)
    pub family_id: String,
    /// Whether this token has been used (for single-use enforcement)
    pub used: bool,
}

/// Boxed future for storage operations.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Trait for OAuth token storage backends.
///
/// Implementations should store tokens by hash and encrypt grant data.
///
/// # Implementations
///
/// - [`MemoryTokenStore`] - In-memory storage (single-request lifetime in Workers)
/// - Future: KV-based storage for cross-request persistence
/// - Future: Durable Objects for strong consistency
pub trait TokenStore: Send + Sync + 'static {
    // =========================================================================
    // Authorization Codes
    // =========================================================================

    /// Store an authorization code.
    ///
    /// The code is stored by its hash, and grant data is encrypted.
    fn store_authorization_code(
        &self,
        code_hash: &str,
        grant: &AuthorizationCodeGrant,
    ) -> BoxFuture<'_, StorageResult<()>>;

    /// Retrieve and consume an authorization code.
    ///
    /// Returns the grant data if found and not expired, then deletes the code.
    /// This ensures single-use of authorization codes.
    fn consume_authorization_code(
        &self,
        code_hash: &str,
    ) -> BoxFuture<'_, StorageResult<AuthorizationCodeGrant>>;

    // =========================================================================
    // Access Tokens
    // =========================================================================

    /// Store access token metadata.
    ///
    /// Note: We don't need to store the full token, only metadata for introspection
    /// and revocation. JWTs are self-contained.
    fn store_access_token(
        &self,
        token_hash: &str,
        data: &AccessTokenData,
    ) -> BoxFuture<'_, StorageResult<()>>;

    /// Get access token data for introspection.
    fn get_access_token(&self, token_hash: &str) -> BoxFuture<'_, StorageResult<AccessTokenData>>;

    /// Revoke an access token.
    fn revoke_access_token(&self, token_hash: &str) -> BoxFuture<'_, StorageResult<()>>;

    // =========================================================================
    // Refresh Tokens
    // =========================================================================

    /// Store a refresh token.
    fn store_refresh_token(
        &self,
        token_hash: &str,
        data: &RefreshTokenData,
    ) -> BoxFuture<'_, StorageResult<()>>;

    /// Get refresh token data.
    fn get_refresh_token(&self, token_hash: &str)
    -> BoxFuture<'_, StorageResult<RefreshTokenData>>;

    /// Mark a refresh token as used (for single-use enforcement).
    fn mark_refresh_token_used(&self, token_hash: &str) -> BoxFuture<'_, StorageResult<()>>;

    /// Revoke a refresh token and all tokens in its family.
    fn revoke_refresh_token_family(&self, family_id: &str) -> BoxFuture<'_, StorageResult<()>>;

    // =========================================================================
    // Cleanup
    // =========================================================================

    /// Clean up expired tokens.
    ///
    /// Returns the number of tokens cleaned up.
    fn cleanup_expired(&self) -> BoxFuture<'_, StorageResult<u64>> {
        Box::pin(async { Ok(0) })
    }
}

/// In-memory token store for testing and single-request scenarios.
///
/// # Note
///
/// In Cloudflare Workers, memory is not persisted across requests.
/// Use KV or Durable Objects for production deployments.
#[derive(Debug, Default)]
pub struct MemoryTokenStore {
    authorization_codes: RwLock<HashMap<String, AuthorizationCodeGrant>>,
    access_tokens: RwLock<HashMap<String, AccessTokenData>>,
    refresh_tokens: RwLock<HashMap<String, RefreshTokenData>>,
}

impl MemoryTokenStore {
    /// Create a new in-memory token store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get current Unix timestamp in seconds.
    fn now_secs() -> u64 {
        (js_sys::Date::now() / 1000.0) as u64
    }
}

impl TokenStore for MemoryTokenStore {
    fn store_authorization_code(
        &self,
        code_hash: &str,
        grant: &AuthorizationCodeGrant,
    ) -> BoxFuture<'_, StorageResult<()>> {
        let code_hash = code_hash.to_string();
        let grant = grant.clone();
        Box::pin(async move {
            let mut codes = self
                .authorization_codes
                .write()
                .map_err(|e| StorageError::Backend(format!("Lock error: {}", e)))?;
            codes.insert(code_hash, grant);
            Ok(())
        })
    }

    fn consume_authorization_code(
        &self,
        code_hash: &str,
    ) -> BoxFuture<'_, StorageResult<AuthorizationCodeGrant>> {
        let code_hash = code_hash.to_string();
        Box::pin(async move {
            let mut codes = self
                .authorization_codes
                .write()
                .map_err(|e| StorageError::Backend(format!("Lock error: {}", e)))?;

            let grant = codes
                .remove(&code_hash)
                .ok_or_else(|| StorageError::NotFound(code_hash.clone()))?;

            // Check expiration
            if Self::now_secs() > grant.expires_at {
                return Err(StorageError::Expired(code_hash));
            }

            Ok(grant)
        })
    }

    fn store_access_token(
        &self,
        token_hash: &str,
        data: &AccessTokenData,
    ) -> BoxFuture<'_, StorageResult<()>> {
        let token_hash = token_hash.to_string();
        let data = data.clone();
        Box::pin(async move {
            let mut tokens = self
                .access_tokens
                .write()
                .map_err(|e| StorageError::Backend(format!("Lock error: {}", e)))?;
            tokens.insert(token_hash, data);
            Ok(())
        })
    }

    fn get_access_token(&self, token_hash: &str) -> BoxFuture<'_, StorageResult<AccessTokenData>> {
        let token_hash = token_hash.to_string();
        Box::pin(async move {
            let tokens = self
                .access_tokens
                .read()
                .map_err(|e| StorageError::Backend(format!("Lock error: {}", e)))?;

            let data = tokens
                .get(&token_hash)
                .ok_or_else(|| StorageError::NotFound(token_hash.clone()))?
                .clone();

            // Check expiration
            if Self::now_secs() > data.expires_at {
                return Err(StorageError::Expired(token_hash));
            }

            Ok(data)
        })
    }

    fn revoke_access_token(&self, token_hash: &str) -> BoxFuture<'_, StorageResult<()>> {
        let token_hash = token_hash.to_string();
        Box::pin(async move {
            let mut tokens = self
                .access_tokens
                .write()
                .map_err(|e| StorageError::Backend(format!("Lock error: {}", e)))?;
            tokens.remove(&token_hash);
            Ok(())
        })
    }

    fn store_refresh_token(
        &self,
        token_hash: &str,
        data: &RefreshTokenData,
    ) -> BoxFuture<'_, StorageResult<()>> {
        let token_hash = token_hash.to_string();
        let data = data.clone();
        Box::pin(async move {
            let mut tokens = self
                .refresh_tokens
                .write()
                .map_err(|e| StorageError::Backend(format!("Lock error: {}", e)))?;
            tokens.insert(token_hash, data);
            Ok(())
        })
    }

    fn get_refresh_token(
        &self,
        token_hash: &str,
    ) -> BoxFuture<'_, StorageResult<RefreshTokenData>> {
        let token_hash = token_hash.to_string();
        Box::pin(async move {
            let tokens = self
                .refresh_tokens
                .read()
                .map_err(|e| StorageError::Backend(format!("Lock error: {}", e)))?;

            let data = tokens
                .get(&token_hash)
                .ok_or_else(|| StorageError::NotFound(token_hash.clone()))?
                .clone();

            // Check expiration
            if Self::now_secs() > data.expires_at {
                return Err(StorageError::Expired(token_hash));
            }

            Ok(data)
        })
    }

    fn mark_refresh_token_used(&self, token_hash: &str) -> BoxFuture<'_, StorageResult<()>> {
        let token_hash = token_hash.to_string();
        Box::pin(async move {
            let mut tokens = self
                .refresh_tokens
                .write()
                .map_err(|e| StorageError::Backend(format!("Lock error: {}", e)))?;

            if let Some(data) = tokens.get_mut(&token_hash) {
                data.used = true;
            }
            Ok(())
        })
    }

    fn revoke_refresh_token_family(&self, family_id: &str) -> BoxFuture<'_, StorageResult<()>> {
        let family_id = family_id.to_string();
        Box::pin(async move {
            let mut tokens = self
                .refresh_tokens
                .write()
                .map_err(|e| StorageError::Backend(format!("Lock error: {}", e)))?;

            tokens.retain(|_, v| v.family_id != family_id);
            Ok(())
        })
    }

    fn cleanup_expired(&self) -> BoxFuture<'_, StorageResult<u64>> {
        Box::pin(async move {
            let now = Self::now_secs();
            let mut count = 0u64;

            // Clean authorization codes
            {
                let mut codes = self
                    .authorization_codes
                    .write()
                    .map_err(|e| StorageError::Backend(format!("Lock error: {}", e)))?;
                let before = codes.len();
                codes.retain(|_, v| v.expires_at > now);
                count += (before - codes.len()) as u64;
            }

            // Clean access tokens
            {
                let mut tokens = self
                    .access_tokens
                    .write()
                    .map_err(|e| StorageError::Backend(format!("Lock error: {}", e)))?;
                let before = tokens.len();
                tokens.retain(|_, v| v.expires_at > now);
                count += (before - tokens.len()) as u64;
            }

            // Clean refresh tokens
            {
                let mut tokens = self
                    .refresh_tokens
                    .write()
                    .map_err(|e| StorageError::Backend(format!("Lock error: {}", e)))?;
                let before = tokens.len();
                tokens.retain(|_, v| v.expires_at > now);
                count += (before - tokens.len()) as u64;
            }

            Ok(count)
        })
    }
}

/// Wrapper for thread-safe token store.
pub type SharedTokenStore = Arc<dyn TokenStore>;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_store_authorization_code() {
        let store = MemoryTokenStore::new();
        let now = (js_sys::Date::now() / 1000.0) as u64;

        let grant = AuthorizationCodeGrant {
            client_id: "test-client".to_string(),
            redirect_uri: "https://example.com/callback".to_string(),
            scopes: vec!["read".to_string()],
            code_challenge: Some("challenge".to_string()),
            code_challenge_method: Some("S256".to_string()),
            subject: "user123".to_string(),
            expires_at: now + 300, // 5 minutes
            nonce: None,
            state: Some("state123".to_string()),
        };

        // Store code
        store
            .store_authorization_code("code_hash_123", &grant)
            .await
            .unwrap();

        // Consume code (should succeed)
        let retrieved = store
            .consume_authorization_code("code_hash_123")
            .await
            .unwrap();
        assert_eq!(retrieved.client_id, "test-client");
        assert_eq!(retrieved.subject, "user123");

        // Consume again (should fail - single use)
        let result = store.consume_authorization_code("code_hash_123").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_memory_store_refresh_token_family_revocation() {
        let store = MemoryTokenStore::new();
        let now = (js_sys::Date::now() / 1000.0) as u64;

        // Store multiple refresh tokens in same family
        for i in 0..3 {
            let data = RefreshTokenData {
                subject: "user123".to_string(),
                client_id: "test-client".to_string(),
                scopes: vec!["read".to_string()],
                expires_at: now + 3600,
                issued_at: now,
                generation: i,
                family_id: "family-abc".to_string(),
                used: false,
            };
            store
                .store_refresh_token(&format!("token_{}", i), &data)
                .await
                .unwrap();
        }

        // Store token in different family
        let other_data = RefreshTokenData {
            subject: "user456".to_string(),
            client_id: "test-client".to_string(),
            scopes: vec!["read".to_string()],
            expires_at: now + 3600,
            issued_at: now,
            generation: 0,
            family_id: "family-xyz".to_string(),
            used: false,
        };
        store
            .store_refresh_token("token_other", &other_data)
            .await
            .unwrap();

        // Revoke family
        store
            .revoke_refresh_token_family("family-abc")
            .await
            .unwrap();

        // Tokens in family should be gone
        assert!(store.get_refresh_token("token_0").await.is_err());
        assert!(store.get_refresh_token("token_1").await.is_err());
        assert!(store.get_refresh_token("token_2").await.is_err());

        // Other family token should still exist
        assert!(store.get_refresh_token("token_other").await.is_ok());
    }
}
