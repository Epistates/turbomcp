//! Core Authentication Types
//!
//! This module contains core types used throughout the TurboMCP authentication system.
//!
//! For authentication context, use `crate::context::AuthContext` (the unified canonical type).

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::time::SystemTime;

use oauth2::RefreshToken;
use serde::{Deserialize, Serialize};

use turbomcp_protocol::{Error as McpError, Result as McpResult};

use super::config::AuthProviderType;

/// User information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    /// User ID
    pub id: String,
    /// Username
    pub username: String,
    /// Email address
    pub email: Option<String>,
    /// Display name
    pub display_name: Option<String>,
    /// Avatar URL
    pub avatar_url: Option<String>,
    /// User metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Token information
#[derive(Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    /// Access token
    pub access_token: String,
    /// Token type (Bearer, etc.)
    pub token_type: String,
    /// Refresh token
    pub refresh_token: Option<String>,
    /// Token expiry in seconds
    pub expires_in: Option<u64>,
    /// Token scope
    pub scope: Option<String>,
}

// Manual Debug impl to prevent token exposure in logs (Sprint 3.6)
impl std::fmt::Debug for TokenInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenInfo")
            .field("access_token", &"[REDACTED]")
            .field("token_type", &self.token_type)
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("expires_in", &self.expires_in)
            .field("scope", &self.scope)
            .finish()
    }
}

/// Authentication provider trait
pub trait AuthProvider: Send + Sync + std::fmt::Debug {
    /// Provider name
    fn name(&self) -> &str;

    /// Provider type
    fn provider_type(&self) -> AuthProviderType;

    /// Authenticate user with credentials
    fn authenticate(
        &self,
        credentials: AuthCredentials,
    ) -> Pin<Box<dyn Future<Output = McpResult<crate::context::AuthContext>> + Send + '_>>;

    /// Validate existing token/session
    fn validate_token(
        &self,
        token: &str,
    ) -> Pin<Box<dyn Future<Output = McpResult<crate::context::AuthContext>> + Send + '_>>;

    /// Refresh access token
    fn refresh_token(
        &self,
        refresh_token: &str,
    ) -> Pin<Box<dyn Future<Output = McpResult<TokenInfo>> + Send + '_>>;

    /// Revoke token/session
    fn revoke_token(&self, token: &str)
    -> Pin<Box<dyn Future<Output = McpResult<()>> + Send + '_>>;

    /// Get user information
    fn get_user_info(
        &self,
        token: &str,
    ) -> Pin<Box<dyn Future<Output = McpResult<UserInfo>> + Send + '_>>;
}

/// Authentication credentials
#[derive(Clone, Serialize, Deserialize)]
pub enum AuthCredentials {
    /// Username and password
    UsernamePassword {
        /// Username
        username: String,
        /// Password
        password: String,
    },
    /// API key
    ApiKey {
        /// API key
        key: String,
    },
    /// OAuth 2.1 authorization code
    OAuth2Code {
        /// Authorization code
        code: String,
        /// State parameter
        state: String,
    },
    /// JWT token
    JwtToken {
        /// JWT token
        token: String,
    },
    /// Custom credentials
    Custom {
        /// Custom credential data
        data: HashMap<String, serde_json::Value>,
    },
}

// Manual Debug impl to prevent credential exposure in logs (Sprint 3.6)
impl std::fmt::Debug for AuthCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthCredentials::UsernamePassword { username, .. } => f
                .debug_struct("AuthCredentials::UsernamePassword")
                .field("username", username)
                .field("password", &"[REDACTED]")
                .finish(),
            AuthCredentials::ApiKey { .. } => f
                .debug_struct("AuthCredentials::ApiKey")
                .field("key", &"[REDACTED]")
                .finish(),
            AuthCredentials::OAuth2Code { state, .. } => f
                .debug_struct("AuthCredentials::OAuth2Code")
                .field("code", &"[REDACTED]")
                .field("state", state)
                .finish(),
            AuthCredentials::JwtToken { .. } => f
                .debug_struct("AuthCredentials::JwtToken")
                .field("token", &"[REDACTED]")
                .finish(),
            AuthCredentials::Custom { .. } => f
                .debug_struct("AuthCredentials::Custom")
                .field("data", &"[REDACTED]")
                .finish(),
        }
    }
}

/// Secure token storage abstraction
pub trait TokenStorage: Send + Sync + std::fmt::Debug {
    /// Store access token securely
    fn store_access_token(
        &self,
        user_id: &str,
        token: &AccessToken,
    ) -> impl Future<Output = McpResult<()>> + Send;

    /// Retrieve access token
    fn get_access_token(
        &self,
        user_id: &str,
    ) -> impl Future<Output = McpResult<Option<AccessToken>>> + Send;

    /// Store refresh token securely (encrypted at rest)
    fn store_refresh_token(
        &self,
        user_id: &str,
        token: &RefreshToken,
    ) -> impl Future<Output = McpResult<()>> + Send;

    /// Retrieve refresh token
    fn get_refresh_token(
        &self,
        user_id: &str,
    ) -> impl Future<Output = McpResult<Option<RefreshToken>>> + Send;

    /// Remove all tokens for user (logout)
    fn revoke_tokens(&self, user_id: &str) -> impl Future<Output = McpResult<()>> + Send;

    /// List all users with stored tokens (for admin)
    fn list_users(&self) -> impl Future<Output = McpResult<Vec<String>>> + Send;
}

/// Secure access token with metadata
#[derive(Clone)]
pub struct AccessToken {
    /// The actual token
    pub(crate) token: String,
    /// Token expiration time
    pub(crate) expires_at: Option<SystemTime>,
    /// Token scopes
    pub(crate) scopes: Vec<String>,
    /// Provider metadata
    pub(crate) metadata: HashMap<String, serde_json::Value>,
}

// Manual Debug impl to prevent token exposure in logs (Sprint 3.6)
impl std::fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccessToken")
            .field("token", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .field("scopes", &self.scopes)
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl AccessToken {
    /// Create a new access token
    #[must_use]
    pub fn new(
        token: String,
        expires_at: Option<SystemTime>,
        scopes: Vec<String>,
        metadata: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            token,
            expires_at,
            scopes,
            metadata,
        }
    }

    /// Get the token value
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Get the token expiration time
    #[must_use]
    pub fn expires_at(&self) -> Option<SystemTime> {
        self.expires_at
    }

    /// Get the token scopes
    #[must_use]
    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }

    /// Get the token metadata
    #[must_use]
    pub fn metadata(&self) -> &HashMap<String, serde_json::Value> {
        &self.metadata
    }
}

/// Authentication middleware trait
pub trait AuthMiddleware: Send + Sync {
    /// Extract authentication token from request
    fn extract_token(
        &self,
        headers: &HashMap<String, String>,
    ) -> impl Future<Output = Option<String>> + Send;

    /// Handle authentication failure
    fn handle_auth_failure(&self, error: McpError) -> impl Future<Output = McpResult<()>> + Send;
}

/// Default authentication middleware
#[derive(Debug, Clone)]
pub struct DefaultAuthMiddleware;

impl AuthMiddleware for DefaultAuthMiddleware {
    fn extract_token(
        &self,
        headers: &HashMap<String, String>,
    ) -> impl Future<Output = Option<String>> + Send {
        let headers = headers.clone();
        async move {
            // Try Authorization header first
            if let Some(auth_header) = headers
                .get("authorization")
                .or_else(|| headers.get("Authorization"))
            {
                if let Some(token) = auth_header.strip_prefix("Bearer ") {
                    return Some(token.to_string());
                }
                if let Some(token) = auth_header.strip_prefix("ApiKey ") {
                    return Some(token.to_string());
                }
            }

            // Try X-API-Key header
            if let Some(api_key) = headers
                .get("x-api-key")
                .or_else(|| headers.get("X-API-Key"))
            {
                return Some(api_key.clone());
            }

            None
        }
    }

    async fn handle_auth_failure(&self, error: McpError) -> McpResult<()> {
        tracing::warn!("Authentication failed: {}", error);
        Err(error)
    }
}
