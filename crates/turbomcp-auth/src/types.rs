//! Core Authentication Types
//!
//! This module contains core types used throughout the TurboMCP authentication system.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use oauth2::RefreshToken;
use serde::{Deserialize, Serialize};

use turbomcp_protocol::{Error as McpError, Result as McpResult};

use super::config::AuthProviderType;

/// Authentication context (LEGACY - use `context::AuthContext` instead)
///
/// NOTE: This is the legacy AuthContext type. New code should use
/// `crate::context::AuthContext` (the unified canonical type).
///
/// This type will be removed in version 3.0.0. Use the unified `context::AuthContext` instead.
/// The `to_unified()` method can help with migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[deprecated(
    since = "2.0.5",
    note = "Use context::AuthContext instead. This type is legacy and will be removed in 3.0.0"
)]
pub struct AuthContext {
    /// User ID
    pub user_id: String,
    /// User information
    pub user: UserInfo,
    /// User roles
    pub roles: Vec<String>,
    /// User permissions
    pub permissions: Vec<String>,
    /// Request ID for replay protection (MCP compliant - NOT session-based)
    ///
    /// Per MCP specification, authentication is stateless. This field is for
    /// request-level binding (DPoP nonces, one-time tokens), not session management.
    pub request_id: String,
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

#[allow(deprecated)]
impl AuthContext {
    /// Convert legacy types::AuthContext to unified context::AuthContext
    pub fn to_unified(&self) -> crate::context::AuthContext {
        crate::context::AuthContext {
            sub: self.user_id.clone(),
            iss: None, // Not present in legacy type
            aud: None, // Not present in legacy type
            exp: self
                .expires_at
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())),
            iat: self
                .authenticated_at
                .duration_since(UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs()),
            nbf: None, // Not present in legacy type
            jti: None, // Not present in legacy type
            user: self.user.clone(),
            roles: self.roles.clone(),
            permissions: self.permissions.clone(),
            scopes: Vec::new(), // Not present in legacy type
            request_id: Some(self.request_id.clone()),
            authenticated_at: self.authenticated_at,
            expires_at: self.expires_at,
            token: self.token.clone(),
            provider: self.provider.clone(),
            #[cfg(feature = "dpop")]
            dpop_jkt: None, // Not present in legacy type
            metadata: self.metadata.clone(),
        }
    }
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
    async fn authenticate(
        &self,
        credentials: AuthCredentials,
    ) -> McpResult<crate::context::AuthContext>;

    /// Validate existing token/session
    async fn validate_token(&self, token: &str) -> McpResult<crate::context::AuthContext>;

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
