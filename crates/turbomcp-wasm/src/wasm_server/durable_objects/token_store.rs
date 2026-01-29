//! Durable Object-backed OAuth token storage.
//!
//! Provides secure, persistent storage for OAuth authorization codes,
//! access tokens, and refresh tokens with automatic expiration.

use serde::{Deserialize, Serialize};
use worker::Env;

/// OAuth token store backed by Cloudflare Durable Objects.
///
/// Stores tokens securely with:
/// - Automatic expiration
/// - Hash-based lookups (tokens stored by hash, not plaintext)
/// - Session binding for refresh tokens
///
/// # Setup
///
/// Configure the Durable Object binding in `wrangler.toml`:
///
/// ```toml
/// [[durable_objects.bindings]]
/// name = "MCP_OAUTH_TOKENS"
/// class_name = "McpOAuthTokenObject"
///
/// [[durable_objects.classes]]
/// name = "McpOAuthTokenObject"
/// class_name = "McpOAuthTokenObject"
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_wasm::wasm_server::durable_objects::DurableObjectTokenStore;
///
/// let store = DurableObjectTokenStore::from_env(&env, "MCP_OAUTH_TOKENS")?;
///
/// // Store an authorization code (expires in 10 minutes)
/// let code_data = OAuthTokenData {
///     token_type: "authorization_code".to_string(),
///     client_id: "my-client".to_string(),
///     user_id: Some("user-123".to_string()),
///     scope: Some("read write".to_string()),
///     code_challenge: Some(code_challenge.to_string()),
///     redirect_uri: Some("https://app.example.com/callback".to_string()),
///     ..Default::default()
/// };
/// store.store_code(&code, &code_data, 600_000).await?;
///
/// // Exchange authorization code
/// let data = store.get_and_delete_code(&code).await?;
///
/// // Store refresh token
/// store.store_refresh_token(&refresh_token, &token_data, 86400_000).await?;
/// ```
#[derive(Clone)]
pub struct DurableObjectTokenStore {
    namespace: String,
    env: Option<Env>,
}

/// OAuth token data stored in the Durable Object.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct OAuthTokenData {
    /// Token type: "authorization_code", "access_token", "refresh_token"
    pub token_type: String,

    /// Client ID the token was issued to
    pub client_id: String,

    /// User ID associated with the token (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// Session ID for refresh token binding
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    /// Scopes granted
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,

    /// PKCE code challenge (for authorization codes)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_challenge: Option<String>,

    /// PKCE code challenge method
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_challenge_method: Option<String>,

    /// Redirect URI (for authorization codes)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_uri: Option<String>,

    /// Token creation timestamp (Unix milliseconds)
    pub created_at: u64,

    /// Token expiration timestamp (Unix milliseconds)
    pub expires_at: u64,

    /// Whether the token has been used (for single-use codes)
    pub used: bool,

    /// Additional custom metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl DurableObjectTokenStore {
    /// Create a new token store with the given DO namespace binding name.
    pub fn new(namespace: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            env: None,
        }
    }

    /// Create a token store from an environment binding.
    pub fn from_env(env: &Env, binding: &str) -> worker::Result<Self> {
        // Validate the binding exists
        let _ = env.durable_object(binding)?;
        Ok(Self {
            namespace: binding.to_string(),
            env: Some(env.clone()),
        })
    }

    /// Set the environment for the store.
    pub fn with_env(mut self, env: Env) -> Self {
        self.env = Some(env);
        self
    }

    /// Store an authorization code.
    ///
    /// Authorization codes are single-use and automatically expire.
    ///
    /// # Arguments
    ///
    /// * `code` - The authorization code
    /// * `data` - Token metadata
    /// * `expires_in_ms` - Time until expiration in milliseconds
    pub async fn store_code(
        &self,
        code: &str,
        data: &OAuthTokenData,
        expires_in_ms: u64,
    ) -> Result<(), TokenStoreError> {
        self.store_token_internal("code", code, data, expires_in_ms)
            .await
    }

    /// Get and delete an authorization code (exchange operation).
    ///
    /// This is atomic - the code is deleted even if retrieval succeeds,
    /// preventing replay attacks.
    ///
    /// # Returns
    ///
    /// The token data if found and not expired, None otherwise.
    pub async fn get_and_delete_code(
        &self,
        code: &str,
    ) -> Result<Option<OAuthTokenData>, TokenStoreError> {
        self.get_and_delete_internal("code", code).await
    }

    /// Store a refresh token.
    ///
    /// Refresh tokens can be revoked and have longer expiration.
    pub async fn store_refresh_token(
        &self,
        token: &str,
        data: &OAuthTokenData,
        expires_in_ms: u64,
    ) -> Result<(), TokenStoreError> {
        self.store_token_internal("refresh", token, data, expires_in_ms)
            .await
    }

    /// Get refresh token data without deleting.
    pub async fn get_refresh_token(
        &self,
        token: &str,
    ) -> Result<Option<OAuthTokenData>, TokenStoreError> {
        self.get_token_internal("refresh", token).await
    }

    /// Revoke a refresh token.
    pub async fn revoke_refresh_token(&self, token: &str) -> Result<bool, TokenStoreError> {
        self.delete_token_internal("refresh", token).await
    }

    /// Revoke all refresh tokens for a user.
    pub async fn revoke_all_for_user(&self, user_id: &str) -> Result<u64, TokenStoreError> {
        #[derive(Serialize)]
        struct RevokeRequest<'a> {
            user_id: &'a str,
        }

        #[derive(Deserialize)]
        struct RevokeResponse {
            revoked: u64,
        }

        let request = RevokeRequest { user_id };
        let response: RevokeResponse = self
            .do_request(user_id, "/tokens/revoke-all-user", Some(&request))
            .await?;

        Ok(response.revoked)
    }

    /// Revoke all refresh tokens for a client.
    pub async fn revoke_all_for_client(&self, client_id: &str) -> Result<u64, TokenStoreError> {
        #[derive(Serialize)]
        struct RevokeRequest<'a> {
            client_id: &'a str,
        }

        #[derive(Deserialize)]
        struct RevokeResponse {
            revoked: u64,
        }

        let request = RevokeRequest { client_id };
        let response: RevokeResponse = self
            .do_request(client_id, "/tokens/revoke-all-client", Some(&request))
            .await?;

        Ok(response.revoked)
    }

    /// Internal: Store a token.
    async fn store_token_internal(
        &self,
        token_type: &str,
        token: &str,
        data: &OAuthTokenData,
        expires_in_ms: u64,
    ) -> Result<(), TokenStoreError> {
        #[derive(Serialize)]
        struct StoreRequest<'a> {
            token_type: &'a str,
            token_hash: String,
            data: &'a OAuthTokenData,
            expires_in_ms: u64,
        }

        let token_hash = hash_token(token);
        let request = StoreRequest {
            token_type,
            token_hash,
            data,
            expires_in_ms,
        };

        // Use client_id as the DO instance key for locality
        self.do_request::<()>(&data.client_id, "/tokens/store", Some(&request))
            .await
    }

    /// Internal: Get a token.
    async fn get_token_internal(
        &self,
        token_type: &str,
        token: &str,
    ) -> Result<Option<OAuthTokenData>, TokenStoreError> {
        #[derive(Serialize)]
        struct GetRequest<'a> {
            token_type: &'a str,
            token_hash: String,
        }

        #[derive(Deserialize)]
        struct GetResponse {
            data: Option<OAuthTokenData>,
        }

        let token_hash = hash_token(token);
        let request = GetRequest {
            token_type,
            token_hash: token_hash.clone(),
        };

        // We don't know the client_id from the token, so use the hash as the key
        let response: GetResponse = self
            .do_request(&token_hash, "/tokens/get", Some(&request))
            .await?;

        Ok(response.data)
    }

    /// Internal: Get and delete a token.
    async fn get_and_delete_internal(
        &self,
        token_type: &str,
        token: &str,
    ) -> Result<Option<OAuthTokenData>, TokenStoreError> {
        #[derive(Serialize)]
        struct GetDeleteRequest<'a> {
            token_type: &'a str,
            token_hash: String,
        }

        #[derive(Deserialize)]
        struct GetDeleteResponse {
            data: Option<OAuthTokenData>,
        }

        let token_hash = hash_token(token);
        let request = GetDeleteRequest {
            token_type,
            token_hash: token_hash.clone(),
        };

        let response: GetDeleteResponse = self
            .do_request(&token_hash, "/tokens/get-and-delete", Some(&request))
            .await?;

        Ok(response.data)
    }

    /// Internal: Delete a token.
    async fn delete_token_internal(
        &self,
        token_type: &str,
        token: &str,
    ) -> Result<bool, TokenStoreError> {
        #[derive(Serialize)]
        struct DeleteRequest<'a> {
            token_type: &'a str,
            token_hash: String,
        }

        #[derive(Deserialize)]
        struct DeleteResponse {
            deleted: bool,
        }

        let token_hash = hash_token(token);
        let request = DeleteRequest {
            token_type,
            token_hash: token_hash.clone(),
        };

        let response: DeleteResponse = self
            .do_request(&token_hash, "/tokens/delete", Some(&request))
            .await?;

        Ok(response.deleted)
    }

    /// Send a request to the Durable Object.
    async fn do_request<T: for<'de> Deserialize<'de>>(
        &self,
        key: &str,
        path: &str,
        body: Option<&impl Serialize>,
    ) -> Result<T, TokenStoreError> {
        let env = self.env.as_ref().ok_or(TokenStoreError::NoEnvironment)?;

        let ns = env
            .durable_object(&self.namespace)
            .map_err(TokenStoreError::Worker)?;
        let id = ns.id_from_name(key).map_err(TokenStoreError::Worker)?;
        let stub = id.get_stub().map_err(TokenStoreError::Worker)?;

        let mut init = worker::RequestInit::new();
        init.with_method(worker::Method::Post);

        if let Some(body) = body {
            let json = serde_json::to_string(body).map_err(TokenStoreError::Serialization)?;
            init.with_body(Some(json.into()));
        }

        let url = format!("https://do-internal{path}");
        let request =
            worker::Request::new_with_init(&url, &init).map_err(TokenStoreError::Worker)?;
        let mut response = stub
            .fetch_with_request(request)
            .await
            .map_err(TokenStoreError::Worker)?;

        let text = response.text().await.map_err(TokenStoreError::Worker)?;
        serde_json::from_str(&text).map_err(TokenStoreError::Deserialization)
    }
}

/// Hash a token using SHA-256 for secure storage.
///
/// Tokens are never stored in plaintext - only their cryptographic hash.
/// This prevents token compromise even if the storage is breached.
fn hash_token(token: &str) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let result = hasher.finalize();

    // Format as hex with tok_ prefix for easy identification
    format!("tok_{:x}", result)
}

/// Error type for token store operations.
#[derive(Debug)]
pub enum TokenStoreError {
    /// No environment has been set
    NoEnvironment,
    /// Worker/DO communication error
    Worker(worker::Error),
    /// Serialization error
    Serialization(serde_json::Error),
    /// Deserialization error
    Deserialization(serde_json::Error),
}

impl std::fmt::Display for TokenStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoEnvironment => write!(f, "No environment set"),
            Self::Worker(e) => write!(f, "Worker error: {e:?}"),
            Self::Serialization(e) => write!(f, "Serialization error: {e}"),
            Self::Deserialization(e) => write!(f, "Deserialization error: {e}"),
        }
    }
}

impl std::error::Error for TokenStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Worker(e) => Some(e),
            Self::Serialization(e) => Some(e),
            Self::Deserialization(e) => Some(e),
            Self::NoEnvironment => None,
        }
    }
}

impl From<worker::Error> for TokenStoreError {
    fn from(e: worker::Error) -> Self {
        Self::Worker(e)
    }
}

// ============================================================================
// Protocol Types
// ============================================================================

/// Request/response types for the token Durable Object.
///
/// Implement a Durable Object class that handles these routes:
///
/// - `POST /tokens/store` - Store a token
/// - `POST /tokens/get` - Get token data
/// - `POST /tokens/get-and-delete` - Get and delete (atomic exchange)
/// - `POST /tokens/delete` - Delete a token
/// - `POST /tokens/revoke-all-user` - Revoke all tokens for a user
/// - `POST /tokens/revoke-all-client` - Revoke all tokens for a client
///
/// # Security Considerations
///
/// - Tokens are stored by hash, never in plaintext
/// - Authorization codes are single-use
/// - Automatic expiration via DO alarm API
/// - Constant-time comparison for token validation
///
/// Protocol types for implementing the Durable Object handler.
///
/// These types are used for documentation and should be implemented
/// by the user in their Durable Object class.
#[allow(dead_code)]
pub mod protocol {
    use super::*;

    /// Request to store a token.
    #[derive(Debug, Serialize, Deserialize)]
    pub struct StoreRequest {
        /// Token type
        pub token_type: String,
        /// Hash of the token
        pub token_hash: String,
        /// Token metadata
        pub data: OAuthTokenData,
        /// Time until expiration in milliseconds
        pub expires_in_ms: u64,
    }

    /// Request to get a token.
    #[derive(Debug, Serialize, Deserialize)]
    pub struct GetRequest {
        /// Token type
        pub token_type: String,
        /// Hash of the token
        pub token_hash: String,
    }

    /// Response from get token.
    #[derive(Debug, Serialize, Deserialize)]
    pub struct GetResponse {
        /// Token data if found
        pub data: Option<OAuthTokenData>,
    }

    /// Response from delete.
    #[derive(Debug, Serialize, Deserialize)]
    pub struct DeleteResponse {
        /// Whether a token was deleted
        pub deleted: bool,
    }

    /// Request to revoke all tokens for a user.
    #[derive(Debug, Serialize, Deserialize)]
    pub struct RevokeAllUserRequest {
        /// User ID
        pub user_id: String,
    }

    /// Request to revoke all tokens for a client.
    #[derive(Debug, Serialize, Deserialize)]
    pub struct RevokeAllClientRequest {
        /// Client ID
        pub client_id: String,
    }

    /// Response from revoke all.
    #[derive(Debug, Serialize, Deserialize)]
    pub struct RevokeAllResponse {
        /// Number of tokens revoked
        pub revoked: u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_store_creation() {
        let store = DurableObjectTokenStore::new("MCP_OAUTH_TOKENS");
        assert_eq!(store.namespace, "MCP_OAUTH_TOKENS");
        assert!(store.env.is_none());
    }

    #[test]
    fn test_token_hashing() {
        let hash1 = hash_token("secret-token-123");
        let hash2 = hash_token("secret-token-123");
        let hash3 = hash_token("different-token");

        // Same token produces same hash
        assert_eq!(hash1, hash2);

        // Different tokens produce different hashes
        assert_ne!(hash1, hash3);

        // Hash has expected format
        assert!(hash1.starts_with("tok_"));
    }

    #[test]
    fn test_oauth_token_data_default() {
        let data = OAuthTokenData::default();
        assert!(data.token_type.is_empty());
        assert!(data.client_id.is_empty());
        assert!(data.user_id.is_none());
        assert!(!data.used);
    }

    #[test]
    fn test_token_store_error_display() {
        let err = TokenStoreError::NoEnvironment;
        assert_eq!(err.to_string(), "No environment set");
    }
}
