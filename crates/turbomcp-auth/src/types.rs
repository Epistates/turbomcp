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
    /// Token lifetime in seconds, as returned by the authorization server (RFC 6749 §5.1).
    pub expires_in: Option<u64>,
    /// Token scope
    pub scope: Option<String>,
    /// Wall-clock instant the token was issued.
    ///
    /// Combined with `expires_in`, this is what makes expiry checks meaningful —
    /// `expires_in` alone is a relative duration at issuance time, not a clock.
    /// `#[serde(default)]` keeps on-disk back-compat with v3.0.x token caches that
    /// did not record this; older entries are treated as "expiry unknown".
    #[serde(default, with = "system_time_millis")]
    pub issued_at: Option<SystemTime>,
}

impl TokenInfo {
    /// Wall-clock instant the token expires, if both `issued_at` and `expires_in` are known.
    #[must_use]
    pub fn expires_at(&self) -> Option<SystemTime> {
        let issued = self.issued_at?;
        let lifetime = self.expires_in?;
        issued.checked_add(std::time::Duration::from_secs(lifetime))
    }

    /// Whether the token is past its expiry, accounting for the supplied clock skew.
    ///
    /// Returns `false` when expiry cannot be determined (legacy tokens missing `issued_at`,
    /// or no `expires_in` from the AS). Callers that need conservative behavior should
    /// pre-check `expires_at().is_some()` and treat unknown as expired.
    #[must_use]
    pub fn is_expired_with_skew(&self, skew: std::time::Duration) -> bool {
        match self.expires_at() {
            Some(expiry) => match expiry.checked_sub(skew) {
                Some(threshold) => SystemTime::now() >= threshold,
                None => true,
            },
            None => false,
        }
    }

    /// Convenience wrapper around [`Self::is_expired_with_skew`] with a 60-second skew.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.is_expired_with_skew(std::time::Duration::from_secs(60))
    }
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
            .field("issued_at", &self.issued_at)
            .field("scope", &self.scope)
            .finish()
    }
}

mod system_time_millis {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &Option<SystemTime>, ser: S) -> Result<S::Ok, S::Error> {
        match value {
            Some(t) => {
                let millis = t
                    .duration_since(UNIX_EPOCH)
                    .map_err(serde::ser::Error::custom)?
                    .as_millis() as u64;
                ser.serialize_some(&millis)
            }
            None => ser.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Option<SystemTime>, D::Error> {
        Option::<u64>::deserialize(de)
            .map(|opt| opt.map(|millis| UNIX_EPOCH + Duration::from_millis(millis)))
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
