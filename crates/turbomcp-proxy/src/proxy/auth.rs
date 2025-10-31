//! Proxy Authentication Module
//!
//! This module provides authentication support for the proxy, enabling:
//! 1. Client authentication (extracting `AuthContext` from incoming requests)
//! 2. Backend JWT signing (generating JWTs for backend servers)
//!
//! # Architecture
//!
//! ```text
//! Client → ProxyService (extract auth) → JWT Signer → Backend (with JWT)
//! ```
//!
//! The proxy acts as an authentication bridge:
//! - Clients authenticate with `OAuth2`, API keys, or existing JWTs
//! - Proxy extracts/validates the `AuthContext`
//! - Proxy signs a new JWT for the backend server
//! - Backend receives properly authenticated requests
//!
//! ## MCP Security Compliance (RFC 9728)
//!
//! This proxy **NEVER** forwards client tokens to backend servers (MCP requirement).
//! Token passthrough is explicitly forbidden as it creates security vulnerabilities:
//!
//! ### Why Token Passthrough is Forbidden
//!
//! - **Security Control Circumvention**: Bypasses rate limiting, validation, monitoring
//! - **Accountability Issues**: Can't distinguish between clients, audit trails break
//! - **Trust Boundary Violations**: Breaks OAuth 2.1 audience validation
//! - **Confused Deputy Attacks**: Downstream APIs may incorrectly trust tokens
//!
//! ### How This Proxy Works Instead
//!
//! 1. Client authenticates with their credentials (`OAuth2`, API key, JWT)
//! 2. Proxy validates and extracts the `AuthContext`
//! 3. Proxy generates a **NEW** JWT specifically for the backend server
//! 4. New JWT has proper `aud` claim binding it to the backend
//! 5. Backend receives properly scoped, backend-specific authentication
//!
//! This design follows MCP security best practices and prevents token theft
//! across service boundaries.

use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use secrecy::ExposeSecret;
use std::time::{SystemTime, UNIX_EPOCH};
use turbomcp_auth::AuthContext;

use crate::error::{ProxyError, ProxyResult};

/// JWT Signer for backend authentication
///
/// The proxy uses this to generate JWTs that backend servers can validate.
/// This enables the proxy to authenticate clients once, then forward authenticated
/// requests to multiple backend servers.
#[derive(Clone)]
pub struct JwtSigner {
    /// Secret key for JWT signing (wrapped in `SecretString` for security)
    secret: secrecy::SecretString,
    /// Algorithm to use (default: HS256)
    algorithm: Algorithm,
    /// Issuer (iss claim)
    issuer: String,
    /// Audience (aud claim) - typically the backend server name
    audience: Option<String>,
    /// Token TTL in seconds (default: 3600 = 1 hour)
    ttl: u64,
}

impl std::fmt::Debug for JwtSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwtSigner")
            .field("secret", &"<redacted>")
            .field("algorithm", &self.algorithm)
            .field("issuer", &self.issuer)
            .field("audience", &self.audience)
            .field("ttl", &self.ttl)
            .finish()
    }
}

impl JwtSigner {
    /// Create a new JWT signer with secret
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_proxy::proxy::auth::JwtSigner;
    ///
    /// let signer = JwtSigner::new(
    ///     "shared-secret-with-backend".to_string(),
    ///     "turbomcp-proxy".to_string()
    /// );
    /// ```
    #[must_use]
    pub fn new(secret: String, issuer: String) -> Self {
        Self {
            secret: secrecy::SecretString::from(secret),
            algorithm: Algorithm::HS256,
            issuer,
            audience: None,
            ttl: 3600, // 1 hour default
        }
    }

    /// Set the algorithm
    #[must_use]
    pub fn with_algorithm(mut self, algorithm: Algorithm) -> Self {
        self.algorithm = algorithm;
        self
    }

    /// Set the audience (backend server name)
    #[must_use]
    pub fn with_audience(mut self, audience: String) -> Self {
        self.audience = Some(audience);
        self
    }

    /// Set the token TTL in seconds
    #[must_use]
    pub fn with_ttl(mut self, ttl: u64) -> Self {
        self.ttl = ttl;
        self
    }

    /// Sign an `AuthContext` into a JWT for backend authentication
    ///
    /// This takes the client's `AuthContext` and generates a JWT that the backend
    /// server can validate. The JWT includes all claims from the `AuthContext`.
    ///
    /// # Errors
    ///
    /// Returns `ProxyError::Auth` if:
    /// - System time is before Unix epoch
    /// - JWT encoding fails (malformed claims or invalid secret)
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use turbomcp_proxy::proxy::auth::JwtSigner;
    /// # use turbomcp_auth::AuthContext;
    /// # let signer = JwtSigner::new("secret".to_string(), "proxy".to_string());
    /// # let auth_context = AuthContext::builder()
    /// #     .subject("user")
    /// #     .user(turbomcp_auth::UserInfo {
    /// #         id: "u".into(), username: "u".into(), email: None,
    /// #         display_name: None, avatar_url: None, metadata: Default::default()
    /// #     })
    /// #     .provider("test")
    /// #     .build().unwrap();
    /// let jwt = signer.sign(&auth_context)?;
    /// // Send JWT to backend in Authorization header: Bearer {jwt}
    /// # Ok::<(), turbomcp_proxy::error::ProxyError>(())
    /// ```
    pub fn sign(&self, auth_context: &AuthContext) -> ProxyResult<String> {
        let now = Self::current_timestamp()?;

        // Create a new AuthContext with updated timing claims for the backend
        let mut backend_context = auth_context.clone();
        backend_context.iss = Some(self.issuer.clone());
        backend_context.aud.clone_from(&self.audience);
        backend_context.iat = Some(now);
        backend_context.exp = Some(now + self.ttl);

        // Convert to JWT claims (this serializes the entire AuthContext)
        let claims = backend_context.to_jwt_claims();

        // Sign the JWT using shared encoding logic
        self.encode_jwt(&claims)
    }

    /// Sign a minimal JWT with just essential claims (for performance)
    ///
    /// This generates a smaller JWT containing only the essential fields.
    /// Use this when you don't need to forward all auth context metadata.
    ///
    /// # Errors
    ///
    /// Returns `ProxyError::Auth` if:
    /// - System time is before Unix epoch
    /// - JWT encoding fails (malformed claims or invalid secret)
    pub fn sign_minimal(&self, sub: &str, roles: &[String]) -> ProxyResult<String> {
        let now = Self::current_timestamp()?;

        let claims = serde_json::json!({
            "sub": sub,
            "roles": roles,
            "iss": self.issuer,
            "aud": self.audience,
            "iat": now,
            "exp": now + self.ttl,
        });

        // Sign the JWT using shared encoding logic
        self.encode_jwt(&claims)
    }

    // ═══════════════════════════════════════════════════
    // PRIVATE HELPERS (DRY)
    // ═══════════════════════════════════════════════════

    /// Get current Unix timestamp
    fn current_timestamp() -> ProxyResult<u64> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| ProxyError::Auth(format!("System time error: {e}")))
            .map(|d| d.as_secs())
    }

    /// Encode JWT claims (shared logic for sign and `sign_minimal`)
    fn encode_jwt(&self, claims: &serde_json::Value) -> ProxyResult<String> {
        let header = Header::new(self.algorithm);
        let encoding_key = EncodingKey::from_secret(self.secret.expose_secret().as_bytes());

        encode(&header, claims, &encoding_key)
            .map_err(|e| ProxyError::Auth(format!("JWT signing failed: {e}")))
    }
}

/// Configuration for proxy authentication
///
/// # MCP Security Compliance
///
/// This configuration enforces MCP security requirements:
/// - **NO token passthrough** - Proxy always generates new JWTs for backends
/// - **Audience binding** - Each backend JWT has proper `aud` claim
/// - **Trust boundaries** - Clear separation between client and backend auth
#[derive(Debug, Clone, Default)]
pub struct ProxyAuthConfig {
    /// JWT signer for backend authentication (required for auth-enabled proxies)
    pub jwt_signer: Option<JwtSigner>,

    /// Whether to require authentication (fail requests without auth)
    pub require_auth: bool,
}

impl ProxyAuthConfig {
    /// Create config with JWT signing for backends
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_proxy::proxy::auth::{ProxyAuthConfig, JwtSigner};
    ///
    /// let signer = JwtSigner::new(
    ///     "backend-secret".to_string(),
    ///     "turbomcp-proxy".to_string()
    /// ).with_audience("backend-server".to_string());
    ///
    /// let config = ProxyAuthConfig::with_jwt_signing(signer);
    /// ```
    #[must_use]
    pub fn with_jwt_signing(jwt_signer: JwtSigner) -> Self {
        Self {
            jwt_signer: Some(jwt_signer),
            require_auth: false,
        }
    }

    /// Require authentication for all requests
    ///
    /// When enabled, requests without valid authentication will be rejected
    /// with HTTP 401 Unauthorized.
    #[must_use]
    pub fn require_auth(mut self) -> Self {
        self.require_auth = true;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::Algorithm;
    use serde_json;
    use std::collections::HashMap;
    use turbomcp_auth::UserInfo;

    fn create_test_auth_context() -> AuthContext {
        AuthContext::builder()
            .subject("test_user")
            .user(UserInfo {
                id: "test_user".to_string(),
                username: "testuser".to_string(),
                email: Some("test@example.com".to_string()),
                display_name: Some("Test User".to_string()),
                avatar_url: None,
                metadata: HashMap::new(),
            })
            .provider("test")
            .roles(vec!["admin".to_string(), "user".to_string()])
            .permissions(vec!["read:data".to_string(), "write:data".to_string()])
            .build()
            .unwrap()
    }

    #[test]
    fn test_jwt_signer_creation() {
        let signer = JwtSigner::new("test-secret".to_string(), "test-proxy".to_string());

        assert_eq!(signer.issuer, "test-proxy");
        assert_eq!(signer.algorithm, Algorithm::HS256);
        assert_eq!(signer.ttl, 3600);
    }

    #[test]
    fn test_jwt_signer_with_options() {
        let signer = JwtSigner::new("test-secret".to_string(), "test-proxy".to_string())
            .with_algorithm(Algorithm::HS512)
            .with_audience("backend-server".to_string())
            .with_ttl(7200);

        assert_eq!(signer.algorithm, Algorithm::HS512);
        assert_eq!(signer.audience, Some("backend-server".to_string()));
        assert_eq!(signer.ttl, 7200);
    }

    #[test]
    fn test_sign_auth_context() {
        let signer = JwtSigner::new("test-secret".to_string(), "test-proxy".to_string())
            .with_audience("backend-server".to_string());

        let auth_context = create_test_auth_context();
        let jwt = signer.sign(&auth_context);

        assert!(jwt.is_ok());
        let jwt_str = jwt.unwrap();
        assert!(!jwt_str.is_empty());
        assert!(jwt_str.contains('.')); // JWT format: header.payload.signature
    }

    #[test]
    fn test_sign_minimal() {
        let signer = JwtSigner::new("test-secret".to_string(), "test-proxy".to_string());

        let jwt = signer.sign_minimal("test_user", &["admin".to_string()]);

        assert!(jwt.is_ok());
        let jwt_str = jwt.unwrap();
        assert!(!jwt_str.is_empty());
    }

    #[test]
    fn test_proxy_auth_config_default() {
        let config = ProxyAuthConfig::default();

        assert!(config.jwt_signer.is_none());
        assert!(!config.require_auth);
    }

    #[test]
    fn test_proxy_auth_config_with_jwt_signing() {
        let signer = JwtSigner::new("test-secret".to_string(), "test-proxy".to_string());

        let config = ProxyAuthConfig::with_jwt_signing(signer).require_auth();

        assert!(config.jwt_signer.is_some());
        assert!(config.require_auth);
    }

    #[test]
    fn test_mcp_security_compliance() {
        use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};

        // Verify that proxy config enforces MCP security requirements
        let signer = JwtSigner::new("secret".to_string(), "proxy".to_string())
            .with_audience("backend".to_string());

        let auth_context = create_test_auth_context();
        let backend_jwt = signer.sign(&auth_context).unwrap();

        // Decode the JWT to verify audience binding
        let key = DecodingKey::from_secret("secret".as_bytes());
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_audience(&["backend"]);
        validation.set_issuer(&["proxy"]);

        let decoded = decode::<serde_json::Value>(&backend_jwt, &key, &validation).unwrap();

        // Verify that audience claim is present (prevents token misuse)
        assert_eq!(decoded.claims["aud"], "backend");
        assert_eq!(decoded.claims["iss"], "proxy");

        // Verify that this is a NEW token (not the client's original)
        assert!(decoded.claims["iat"].is_number());
        assert!(decoded.claims["exp"].is_number());
    }

    #[test]
    fn test_jwt_roundtrip() {
        use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
        use serde_json;

        let secret = "test-secret";
        let signer = JwtSigner::new(secret.to_string(), "test-proxy".to_string())
            .with_audience("backend-server".to_string());

        let auth_context = create_test_auth_context();
        let jwt = signer.sign(&auth_context).unwrap();

        // Verify the JWT can be decoded
        let decoding_key = DecodingKey::from_secret(secret.as_bytes());
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_audience(&["backend-server"]);
        validation.set_issuer(&["test-proxy"]);

        let decoded = decode::<serde_json::Value>(&jwt, &decoding_key, &validation);
        assert!(decoded.is_ok());

        let claims = decoded.unwrap().claims;
        assert_eq!(claims["sub"], "test_user");
        assert_eq!(claims["iss"], "test-proxy");
        assert_eq!(claims["aud"], "backend-server");
    }
}
