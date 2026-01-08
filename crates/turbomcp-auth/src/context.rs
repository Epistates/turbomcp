//! Unified Authentication Context
//!
//! This module provides the canonical `AuthContext` type used across TurboMCP.
//! It serves as both the internal authentication representation AND the JWT claims structure.
//!
//! # Design Principles
//!
//! - **Single Source of Truth**: ONE auth context type, used everywhere
//! - **Standards-Compliant**: RFC 7519 (JWT), OAuth 2.1, RFC 9449 (DPoP)
//! - **Feature-Gated**: Zero-cost abstractions - no overhead for unused features
//! - **Extensible**: Custom claims via metadata HashMap

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

// Import from existing types module to avoid duplication
pub use crate::types::{TokenInfo, UserInfo};

/// Validation configuration for AuthContext
#[derive(Debug, Clone)]
pub struct ValidationConfig {
    /// Expected issuer (iss claim)
    pub issuer: Option<String>,
    /// Expected audience (aud claim)
    pub audience: Option<String>,
    /// Clock skew tolerance for exp/nbf validation
    pub leeway: Duration,
    /// Validate expiration (exp claim)
    pub validate_exp: bool,
    /// Validate not-before (nbf claim)
    pub validate_nbf: bool,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            issuer: None,
            audience: None,
            leeway: Duration::from_secs(60), // 60 second clock skew tolerance
            validate_exp: true,
            validate_nbf: true,
        }
    }
}

/// Unified authentication context containing user identity, claims, and session metadata.
///
/// This type serves as both:
/// - The internal authentication representation
/// - The JWT claims structure (via `to_jwt_claims` / `from_jwt_claims`)
///
/// # Standard JWT Claims (RFC 7519)
///
/// - `sub`: Subject (user ID)
/// - `iss`: Issuer (who issued the token)
/// - `aud`: Audience (who the token is for)
/// - `exp`: Expiration time (Unix timestamp)
/// - `iat`: Issued at (Unix timestamp)
/// - `nbf`: Not before (Unix timestamp)
/// - `jti`: JWT ID (unique identifier)
///
/// # Extended Claims
///
/// - `user`: Full user information
/// - `roles`: RBAC roles
/// - `permissions`: Fine-grained permissions
/// - `scopes`: OAuth scopes
/// - `request_id`: Request identifier for replay protection (NOT session-based)
/// - `provider`: Auth provider identifier
/// - `metadata`: Custom claims
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_auth::context::{AuthContext, AuthContextBuilder};
///
/// let ctx = AuthContext::builder()
///     .subject("user123")
///     .user(user_info)
///     .roles(vec!["admin".into(), "user".into()])
///     .permissions(vec!["read:posts".into(), "write:posts".into()])
///     .build();
///
/// // Check authorization
/// if ctx.has_role("admin") && ctx.has_permission("write:posts") {
///     // Allow action
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthContext {
    // ═══════════════════════════════════════════════════
    // STANDARD JWT CLAIMS (RFC 7519)
    // ═══════════════════════════════════════════════════
    /// Subject (typically user ID)
    pub sub: String,

    /// Issuer (who issued this token)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,

    /// Audience (who this token is for)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aud: Option<String>,

    /// Expiration time (Unix timestamp)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<u64>,

    /// Issued at (Unix timestamp)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<u64>,

    /// Not before (Unix timestamp)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nbf: Option<u64>,

    /// JWT ID (unique identifier)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,

    // ═══════════════════════════════════════════════════
    // EXTENDED IDENTITY CLAIMS
    // ═══════════════════════════════════════════════════
    /// Full user information
    pub user: UserInfo,

    /// RBAC roles (e.g., ["admin", "user"])
    #[serde(default)]
    pub roles: Vec<String>,

    /// Fine-grained permissions (e.g., ["read:posts", "write:posts"])
    #[serde(default)]
    pub permissions: Vec<String>,

    /// OAuth scopes (e.g., ["openid", "email", "profile"])
    #[serde(default)]
    pub scopes: Vec<String>,

    // ═══════════════════════════════════════════════════
    // REQUEST & TOKEN METADATA
    // ═══════════════════════════════════════════════════
    /// Request ID for nonce/replay protection (MCP compliant - NOT session-based)
    ///
    /// Per MCP security requirements, servers MUST NOT use sessions for authentication.
    /// This field is for request-level binding (DPoP nonces, one-time tokens, etc.),
    /// not session management. Each request must include valid credentials.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,

    /// When authentication occurred
    #[serde(with = "systemtime_serde")]
    pub authenticated_at: SystemTime,

    /// When this context expires (may differ from JWT exp)
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "systemtime_serde_opt"
    )]
    pub expires_at: Option<SystemTime>,

    /// Token information (access + refresh tokens)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<TokenInfo>,

    /// Auth provider (e.g., "oauth2:google", "api_key", "jwt:internal")
    pub provider: String,

    // ═══════════════════════════════════════════════════
    // DPOP BINDING (RFC 9449) - Feature-gated
    // ═══════════════════════════════════════════════════
    #[cfg(feature = "dpop")]
    #[serde(skip_serializing_if = "Option::is_none")]
    /// DPoP JWK thumbprint for token binding
    pub dpop_jkt: Option<String>,

    // ═══════════════════════════════════════════════════
    // CUSTOM CLAIMS (extensibility)
    // ═══════════════════════════════════════════════════
    /// Custom metadata (tenant_id, org_id, etc.)
    #[serde(flatten)]
    pub metadata: HashMap<String, Value>,
}

// ═══════════════════════════════════════════════════════════
// SYSTEMTIME SERDE HELPERS
// ═══════════════════════════════════════════════════════════

mod systemtime_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(time: &SystemTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let since_epoch = time
            .duration_since(UNIX_EPOCH)
            .map_err(serde::ser::Error::custom)?;
        serializer.serialize_u64(since_epoch.as_secs())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<SystemTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(UNIX_EPOCH + Duration::from_secs(secs))
    }
}

mod systemtime_serde_opt {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(time: &Option<SystemTime>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match time {
            Some(t) => {
                let since_epoch = t
                    .duration_since(UNIX_EPOCH)
                    .map_err(serde::ser::Error::custom)?;
                serializer.serialize_some(&since_epoch.as_secs())
            }
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<SystemTime>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<u64> = Option::deserialize(deserializer)?;
        Ok(opt.map(|secs| UNIX_EPOCH + Duration::from_secs(secs)))
    }
}

// ═══════════════════════════════════════════════════════════
// AUTHCONTEXT IMPLEMENTATION
// ═══════════════════════════════════════════════════════════

impl AuthContext {
    /// Create builder for constructing auth context
    pub fn builder() -> AuthContextBuilder {
        AuthContextBuilder::default()
    }

    // ═══════════════════════════════════════════════════
    // JWT SERIALIZATION (for token generation)
    // ═══════════════════════════════════════════════════

    /// Convert to JWT claims (for signing)
    ///
    /// Serializes the entire AuthContext into a JSON value suitable for JWT encoding.
    /// Standard JWT claims (sub, iss, aud, exp, iat, nbf, jti) are included at the top level.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let claims = auth_ctx.to_jwt_claims();
    /// let token = jwt_encoder.encode(&claims)?;
    /// ```
    pub fn to_jwt_claims(&self) -> Value {
        serde_json::to_value(self).expect("AuthContext serialization should never fail")
    }

    /// Create from JWT claims (after validation)
    ///
    /// Deserializes a validated JWT claims object into an AuthContext.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Required fields are missing (sub, user, provider)
    /// - Field types don't match expected types
    /// - Invalid timestamps
    pub fn from_jwt_claims(claims: Value) -> Result<Self, AuthError> {
        serde_json::from_value(claims).map_err(|e| AuthError::InvalidClaims(e.to_string()))
    }

    // ═══════════════════════════════════════════════════
    // VALIDATION METHODS
    // ═══════════════════════════════════════════════════

    /// Check if token is expired
    ///
    /// Uses `expires_at` field if present, otherwise falls back to `exp` claim.
    pub fn is_expired(&self) -> bool {
        // First check expires_at (internal expiration)
        if let Some(expires_at) = self.expires_at
            && SystemTime::now() > expires_at
        {
            return true;
        }

        // Fall back to exp claim (JWT expiration)
        if let Some(exp) = self.exp {
            let exp_time = UNIX_EPOCH + Duration::from_secs(exp);
            if SystemTime::now() > exp_time {
                return true;
            }
        }

        false
    }

    /// Validate all fields (exp, nbf, aud, iss)
    ///
    /// Performs comprehensive validation according to RFC 7519.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Token is expired (with leeway)
    /// - Token not yet valid (nbf with leeway)
    /// - Audience mismatch
    /// - Issuer mismatch
    pub fn validate(&self, config: &ValidationConfig) -> Result<(), AuthError> {
        let now = SystemTime::now();

        // Validate expiration (exp)
        if config.validate_exp
            && let Some(exp) = self.exp
        {
            let exp_time = UNIX_EPOCH + Duration::from_secs(exp);
            let exp_with_leeway = exp_time + config.leeway;
            if now > exp_with_leeway {
                return Err(AuthError::TokenExpired);
            }
        }

        // Validate not-before (nbf)
        if config.validate_nbf
            && let Some(nbf) = self.nbf
        {
            let nbf_time = UNIX_EPOCH + Duration::from_secs(nbf);
            if nbf_time > now + config.leeway {
                return Err(AuthError::TokenNotYetValid);
            }
        }

        // Validate audience (aud)
        if let Some(ref expected_aud) = config.audience {
            match &self.aud {
                Some(aud) if aud == expected_aud => {}
                _ => return Err(AuthError::InvalidAudience),
            }
        }

        // Validate issuer (iss)
        if let Some(ref expected_iss) = config.issuer {
            match &self.iss {
                Some(iss) if iss == expected_iss => {}
                _ => return Err(AuthError::InvalidIssuer),
            }
        }

        Ok(())
    }

    // ═══════════════════════════════════════════════════
    // AUTHORIZATION HELPERS
    // ═══════════════════════════════════════════════════

    /// Check if user has specific role
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }

    /// Check if user has any of the roles
    pub fn has_any_role(&self, roles: &[&str]) -> bool {
        roles.iter().any(|r| self.has_role(r))
    }

    /// Check if user has all of the roles
    pub fn has_all_roles(&self, roles: &[&str]) -> bool {
        roles.iter().all(|r| self.has_role(r))
    }

    /// Check if user has specific permission
    pub fn has_permission(&self, perm: &str) -> bool {
        self.permissions.iter().any(|p| p == perm)
    }

    /// Check if user has any of the permissions
    pub fn has_any_permission(&self, perms: &[&str]) -> bool {
        perms.iter().any(|p| self.has_permission(p))
    }

    /// Check if user has all of the permissions
    pub fn has_all_permissions(&self, perms: &[&str]) -> bool {
        perms.iter().all(|p| self.has_permission(p))
    }

    /// Check if token has specific scope
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }

    /// Check if token has any of the scopes
    pub fn has_any_scope(&self, scopes: &[&str]) -> bool {
        scopes.iter().any(|s| self.has_scope(s))
    }

    /// Check if token has all of the scopes
    pub fn has_all_scopes(&self, scopes: &[&str]) -> bool {
        scopes.iter().all(|s| self.has_scope(s))
    }

    /// Get custom metadata value
    ///
    /// Deserializes a custom metadata field into the specified type.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(tenant_id) = auth_ctx.get_metadata::<String>("tenant_id") {
    ///     println!("Tenant: {}", tenant_id);
    /// }
    /// ```
    pub fn get_metadata<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.metadata
            .get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    // ═══════════════════════════════════════════════════
    // DPOP SUPPORT (feature-gated)
    // ═══════════════════════════════════════════════════

    #[cfg(feature = "dpop")]
    /// Validate DPoP proof (RFC 9449)
    ///
    /// Verifies that the DPoP proof matches the bound JWK thumbprint.
    pub fn validate_dpop_proof(&self, proof: &DpopProof) -> Result<(), AuthError> {
        match &self.dpop_jkt {
            Some(jkt) if jkt == &proof.jkt => Ok(()),
            Some(_) => Err(AuthError::DpopMismatch),
            None => Err(AuthError::DpopRequired),
        }
    }
}

// ═══════════════════════════════════════════════════════════
// BUILDER PATTERN
// ═══════════════════════════════════════════════════════════

/// Builder for constructing `AuthContext`
///
/// Provides a fluent API for building auth contexts with validation.
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_auth::context::AuthContext;
///
/// let ctx = AuthContext::builder()
///     .subject("user123")
///     .user(user_info)
///     .roles(vec!["admin".into()])
///     .permissions(vec!["read:posts".into()])
///     .provider("oauth2:google")
///     .build();
/// ```
#[derive(Default)]
pub struct AuthContextBuilder {
    sub: Option<String>,
    iss: Option<String>,
    aud: Option<String>,
    exp: Option<u64>,
    iat: Option<u64>,
    nbf: Option<u64>,
    jti: Option<String>,
    user: Option<UserInfo>,
    roles: Vec<String>,
    permissions: Vec<String>,
    scopes: Vec<String>,
    request_id: Option<String>,
    authenticated_at: Option<SystemTime>,
    expires_at: Option<SystemTime>,
    token: Option<TokenInfo>,
    provider: Option<String>,
    #[cfg(feature = "dpop")]
    dpop_jkt: Option<String>,
    metadata: HashMap<String, Value>,
}

impl AuthContextBuilder {
    /// Set subject (user ID)
    pub fn subject(mut self, sub: impl Into<String>) -> Self {
        self.sub = Some(sub.into());
        self
    }

    /// Set issuer
    pub fn iss(mut self, iss: impl Into<String>) -> Self {
        self.iss = Some(iss.into());
        self
    }

    /// Set audience
    pub fn aud(mut self, aud: impl Into<String>) -> Self {
        self.aud = Some(aud.into());
        self
    }

    /// Set expiration (Unix timestamp)
    pub fn exp(mut self, exp: u64) -> Self {
        self.exp = Some(exp);
        self
    }

    /// Set issued at (Unix timestamp)
    pub fn iat(mut self, iat: u64) -> Self {
        self.iat = Some(iat);
        self
    }

    /// Set not before (Unix timestamp)
    pub fn nbf(mut self, nbf: u64) -> Self {
        self.nbf = Some(nbf);
        self
    }

    /// Set JWT ID
    pub fn jti(mut self, jti: impl Into<String>) -> Self {
        self.jti = Some(jti.into());
        self
    }

    /// Set user information
    pub fn user(mut self, user: UserInfo) -> Self {
        self.user = Some(user);
        self
    }

    /// Set roles
    pub fn roles(mut self, roles: Vec<String>) -> Self {
        self.roles = roles;
        self
    }

    /// Add a single role
    pub fn role(mut self, role: impl Into<String>) -> Self {
        self.roles.push(role.into());
        self
    }

    /// Set permissions
    pub fn permissions(mut self, permissions: Vec<String>) -> Self {
        self.permissions = permissions;
        self
    }

    /// Add a single permission
    pub fn permission(mut self, permission: impl Into<String>) -> Self {
        self.permissions.push(permission.into());
        self
    }

    /// Set scopes
    pub fn scopes(mut self, scopes: Vec<String>) -> Self {
        self.scopes = scopes;
        self
    }

    /// Add a single scope
    pub fn scope(mut self, scope: impl Into<String>) -> Self {
        self.scopes.push(scope.into());
        self
    }

    /// Set request ID for nonce/replay protection
    ///
    /// This is used for request-level binding (DPoP nonces, one-time request tokens),
    /// NOT for session management. MCP requires stateless authentication where each
    /// request includes valid credentials.
    pub fn request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    /// Set authenticated at timestamp
    pub fn authenticated_at(mut self, authenticated_at: SystemTime) -> Self {
        self.authenticated_at = Some(authenticated_at);
        self
    }

    /// Set expires at timestamp
    pub fn expires_at(mut self, expires_at: SystemTime) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Set token information
    pub fn token(mut self, token: TokenInfo) -> Self {
        self.token = Some(token);
        self
    }

    /// Set auth provider
    pub fn provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    /// Set DPoP JWK thumbprint (requires dpop feature)
    #[cfg(feature = "dpop")]
    pub fn dpop_jkt(mut self, jkt: impl Into<String>) -> Self {
        self.dpop_jkt = Some(jkt.into());
        self
    }

    /// Add custom metadata
    pub fn metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    /// Build the `AuthContext`
    ///
    /// # Errors
    ///
    /// Returns error if required fields are missing:
    /// - `sub` (subject)
    /// - `user` (user information)
    /// - `provider` (auth provider)
    pub fn build(self) -> Result<AuthContext, AuthError> {
        let sub = self.sub.ok_or(AuthError::MissingField("sub"))?;
        let user = self.user.ok_or(AuthError::MissingField("user"))?;
        let provider = self.provider.ok_or(AuthError::MissingField("provider"))?;
        let authenticated_at = self.authenticated_at.unwrap_or_else(SystemTime::now);

        Ok(AuthContext {
            sub,
            iss: self.iss,
            aud: self.aud,
            exp: self.exp,
            iat: self.iat,
            nbf: self.nbf,
            jti: self.jti,
            user,
            roles: self.roles,
            permissions: self.permissions,
            scopes: self.scopes,
            request_id: self.request_id,
            authenticated_at,
            expires_at: self.expires_at,
            token: self.token,
            provider,
            #[cfg(feature = "dpop")]
            dpop_jkt: self.dpop_jkt,
            metadata: self.metadata,
        })
    }
}

// ═══════════════════════════════════════════════════════════
// ERROR TYPES
// ═══════════════════════════════════════════════════════════

/// Authentication errors
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("Invalid claims: {0}")]
    InvalidClaims(String),

    #[error("Token expired")]
    TokenExpired,

    #[error("Token not yet valid")]
    TokenNotYetValid,

    #[error("Invalid audience")]
    InvalidAudience,

    #[error("Invalid issuer")]
    InvalidIssuer,

    #[error("Missing required field: {0}")]
    MissingField(&'static str),

    #[cfg(feature = "dpop")]
    #[error("DPoP proof mismatch")]
    DpopMismatch,

    #[cfg(feature = "dpop")]
    #[error("DPoP proof required but not provided")]
    DpopRequired,
}

// ═══════════════════════════════════════════════════════════
// DPOP TYPES (feature-gated)
// ═══════════════════════════════════════════════════════════

#[cfg(feature = "dpop")]
/// DPoP proof for token binding (RFC 9449)
pub struct DpopProof {
    /// JWK thumbprint
    pub jkt: String,
}

// ═══════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_user() -> UserInfo {
        UserInfo {
            id: "user123".to_string(),
            username: "testuser".to_string(),
            email: Some("test@example.com".to_string()),
            display_name: Some("Test User".to_string()),
            avatar_url: None,
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn test_builder_minimal() {
        let user = create_test_user();
        let ctx = AuthContext::builder()
            .subject("user123")
            .user(user)
            .provider("test")
            .build()
            .unwrap();

        assert_eq!(ctx.sub, "user123");
        assert_eq!(ctx.provider, "test");
        assert!(ctx.roles.is_empty());
        assert!(ctx.permissions.is_empty());
    }

    #[test]
    fn test_builder_full() {
        let user = create_test_user();
        let ctx = AuthContext::builder()
            .subject("user123")
            .iss("test-issuer")
            .aud("test-audience")
            .user(user)
            .roles(vec!["admin".to_string(), "user".to_string()])
            .permissions(vec!["read:posts".to_string()])
            .scopes(vec!["openid".to_string(), "email".to_string()])
            .provider("oauth2:test")
            .build()
            .unwrap();

        assert_eq!(ctx.sub, "user123");
        assert_eq!(ctx.iss, Some("test-issuer".to_string()));
        assert_eq!(ctx.aud, Some("test-audience".to_string()));
        assert_eq!(ctx.roles.len(), 2);
        assert_eq!(ctx.permissions.len(), 1);
        assert_eq!(ctx.scopes.len(), 2);
    }

    #[test]
    fn test_is_expired() {
        let user = create_test_user();

        // Not expired (future expiration)
        let future = SystemTime::now() + Duration::from_secs(3600);
        let ctx = AuthContext::builder()
            .subject("user123")
            .user(user.clone())
            .provider("test")
            .expires_at(future)
            .build()
            .unwrap();
        assert!(!ctx.is_expired());

        // Expired (past expiration)
        let past = SystemTime::now() - Duration::from_secs(3600);
        let ctx = AuthContext::builder()
            .subject("user123")
            .user(user)
            .provider("test")
            .expires_at(past)
            .build()
            .unwrap();
        assert!(ctx.is_expired());
    }

    #[test]
    fn test_has_role() {
        let user = create_test_user();
        let ctx = AuthContext::builder()
            .subject("user123")
            .user(user)
            .provider("test")
            .roles(vec!["admin".to_string(), "user".to_string()])
            .build()
            .unwrap();

        assert!(ctx.has_role("admin"));
        assert!(ctx.has_role("user"));
        assert!(!ctx.has_role("superuser"));
    }

    #[test]
    fn test_has_any_role() {
        let user = create_test_user();
        let ctx = AuthContext::builder()
            .subject("user123")
            .user(user)
            .provider("test")
            .roles(vec!["admin".to_string(), "user".to_string()])
            .build()
            .unwrap();

        assert!(ctx.has_any_role(&["admin", "superuser"]));
        assert!(ctx.has_any_role(&["user", "guest"]));
        assert!(!ctx.has_any_role(&["superuser", "guest"]));
    }

    #[test]
    fn test_has_all_roles() {
        let user = create_test_user();
        let ctx = AuthContext::builder()
            .subject("user123")
            .user(user)
            .provider("test")
            .roles(vec!["admin".to_string(), "user".to_string()])
            .build()
            .unwrap();

        assert!(ctx.has_all_roles(&["admin", "user"]));
        assert!(ctx.has_all_roles(&["admin"]));
        assert!(!ctx.has_all_roles(&["admin", "user", "superuser"]));
    }

    #[test]
    fn test_has_permission() {
        let user = create_test_user();
        let ctx = AuthContext::builder()
            .subject("user123")
            .user(user)
            .provider("test")
            .permissions(vec!["read:posts".to_string(), "write:posts".to_string()])
            .build()
            .unwrap();

        assert!(ctx.has_permission("read:posts"));
        assert!(ctx.has_permission("write:posts"));
        assert!(!ctx.has_permission("delete:posts"));
    }

    #[test]
    fn test_has_scope() {
        let user = create_test_user();
        let ctx = AuthContext::builder()
            .subject("user123")
            .user(user)
            .provider("test")
            .scopes(vec!["openid".to_string(), "email".to_string()])
            .build()
            .unwrap();

        assert!(ctx.has_scope("openid"));
        assert!(ctx.has_scope("email"));
        assert!(!ctx.has_scope("profile"));
    }

    #[test]
    fn test_jwt_serialization() {
        let user = create_test_user();
        let ctx = AuthContext::builder()
            .subject("user123")
            .iss("test-issuer")
            .user(user)
            .provider("test")
            .roles(vec!["admin".to_string()])
            .build()
            .unwrap();

        // Serialize to JWT claims
        let claims = ctx.to_jwt_claims();
        assert!(claims.is_object());

        // Deserialize back
        let ctx2 = AuthContext::from_jwt_claims(claims).unwrap();
        assert_eq!(ctx2.sub, ctx.sub);
        assert_eq!(ctx2.iss, ctx.iss);
        assert_eq!(ctx2.roles, ctx.roles);
    }

    #[test]
    fn test_validation_expired() {
        let user = create_test_user();
        let past_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 3600;

        let ctx = AuthContext::builder()
            .subject("user123")
            .user(user)
            .provider("test")
            .exp(past_timestamp)
            .build()
            .unwrap();

        let config = ValidationConfig::default();
        let result = ctx.validate(&config);
        assert!(matches!(result, Err(AuthError::TokenExpired)));
    }

    #[test]
    fn test_validation_not_yet_valid() {
        let user = create_test_user();
        let future_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600;

        let ctx = AuthContext::builder()
            .subject("user123")
            .user(user)
            .provider("test")
            .nbf(future_timestamp)
            .build()
            .unwrap();

        let config = ValidationConfig::default();
        let result = ctx.validate(&config);
        assert!(matches!(result, Err(AuthError::TokenNotYetValid)));
    }

    #[test]
    fn test_validation_audience() {
        let user = create_test_user();
        let ctx = AuthContext::builder()
            .subject("user123")
            .user(user)
            .provider("test")
            .aud("wrong-audience")
            .build()
            .unwrap();

        let config = ValidationConfig {
            audience: Some("expected-audience".to_string()),
            ..Default::default()
        };

        let result = ctx.validate(&config);
        assert!(matches!(result, Err(AuthError::InvalidAudience)));
    }

    #[test]
    fn test_metadata() {
        let user = create_test_user();
        let ctx = AuthContext::builder()
            .subject("user123")
            .user(user)
            .provider("test")
            .metadata("tenant_id", Value::String("tenant123".to_string()))
            .metadata("org_id", Value::Number(42.into()))
            .build()
            .unwrap();

        let tenant_id: Option<String> = ctx.get_metadata("tenant_id");
        assert_eq!(tenant_id, Some("tenant123".to_string()));

        let org_id: Option<i64> = ctx.get_metadata("org_id");
        assert_eq!(org_id, Some(42));

        let missing: Option<String> = ctx.get_metadata("missing");
        assert_eq!(missing, None);
    }
}
