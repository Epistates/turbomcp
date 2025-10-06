//! Core Authentication Types
//!
//! This module contains core types used throughout the TurboMCP authentication system.

use std::collections::HashMap;
use std::time::SystemTime;

use async_trait::async_trait;
use oauth2::RefreshToken;
use serde::{Deserialize, Serialize};

use turbomcp_core::{Error as McpError, Result as McpResult};

use super::config::AuthProviderType;

/// Authentication context containing user information and session data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthContext {
    /// User ID
    pub user_id: String,
    /// User information
    pub user: UserInfo,
    /// User roles
    pub roles: Vec<String>,
    /// User permissions
    pub permissions: Vec<String>,
    /// Session ID
    pub session_id: String,
    /// Token information
    pub token: Option<TokenInfo>,
    /// Authentication provider used
    pub provider: String,
    /// Authentication timestamp
    pub authenticated_at: SystemTime,
    /// Token expiry time
    pub expires_at: Option<SystemTime>,
    /// Additional metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Authentication provider trait
#[async_trait]
pub trait AuthProvider: Send + Sync + std::fmt::Debug {
    /// Provider name
    fn name(&self) -> &str;

    /// Provider type
    fn provider_type(&self) -> AuthProviderType;

    /// Authenticate user with credentials
    async fn authenticate(&self, credentials: AuthCredentials) -> McpResult<AuthContext>;

    /// Validate existing token/session
    async fn validate_token(&self, token: &str) -> McpResult<AuthContext>;

    /// Refresh access token
    async fn refresh_token(&self, refresh_token: &str) -> McpResult<TokenInfo>;

    /// Revoke token/session
    async fn revoke_token(&self, token: &str) -> McpResult<()>;

    /// Get user information
    async fn get_user_info(&self, token: &str) -> McpResult<UserInfo>;
}

/// Authentication credentials
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// OAuth 2.0 authorization code
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

/// Secure token storage abstraction
#[async_trait]
pub trait TokenStorage: Send + Sync + std::fmt::Debug {
    /// Store access token securely
    async fn store_access_token(&self, user_id: &str, token: &AccessToken) -> McpResult<()>;

    /// Retrieve access token
    async fn get_access_token(&self, user_id: &str) -> McpResult<Option<AccessToken>>;

    /// Store refresh token securely (encrypted at rest)
    async fn store_refresh_token(&self, user_id: &str, token: &RefreshToken) -> McpResult<()>;

    /// Retrieve refresh token
    async fn get_refresh_token(&self, user_id: &str) -> McpResult<Option<RefreshToken>>;

    /// Remove all tokens for user (logout)
    async fn revoke_tokens(&self, user_id: &str) -> McpResult<()>;

    /// List all users with stored tokens (for admin)
    async fn list_users(&self) -> McpResult<Vec<String>>;
}

/// Secure access token with metadata
#[derive(Debug, Clone)]
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

impl AccessToken {
    /// Create a new access token
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
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Get the token expiration time
    pub fn expires_at(&self) -> Option<SystemTime> {
        self.expires_at
    }

    /// Get the token scopes
    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }

    /// Get the token metadata
    pub fn metadata(&self) -> &HashMap<String, serde_json::Value> {
        &self.metadata
    }
}

/// Authentication middleware trait
#[async_trait]
pub trait AuthMiddleware: Send + Sync {
    /// Extract authentication token from request
    async fn extract_token(&self, headers: &HashMap<String, String>) -> Option<String>;

    /// Handle authentication failure
    async fn handle_auth_failure(&self, error: McpError) -> McpResult<()>;
}

/// Default authentication middleware
#[derive(Debug, Clone)]
pub struct DefaultAuthMiddleware;

#[async_trait]
impl AuthMiddleware for DefaultAuthMiddleware {
    async fn extract_token(&self, headers: &HashMap<String, String>) -> Option<String> {
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

    async fn handle_auth_failure(&self, error: McpError) -> McpResult<()> {
        tracing::warn!("Authentication failed: {}", error);
        Err(Box::new(error))
    }
}
