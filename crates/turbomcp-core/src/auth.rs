//! Authentication traits and types for MCP servers.
//!
//! This module provides platform-adaptive authentication primitives that work
//! on both native and WASM targets. The traits use [`MaybeSend`] and [`MaybeSync`]
//! bounds for portability.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                    turbomcp-core                         │
//! │  ┌─────────────┐  ┌──────────────┐  ┌───────────────┐  │
//! │  │ Credential  │  │ Principal    │  │ Authenticator │  │
//! │  │ (enum)      │  │ (validated)  │  │ (trait)       │  │
//! │  └─────────────┘  └──────────────┘  └───────────────┘  │
//! └─────────────────────────────────────────────────────────┘
//!                            │
//!            ┌───────────────┴───────────────┐
//!            ▼                               ▼
//!    ┌───────────────┐               ┌───────────────┐
//!    │ turbomcp-auth │               │ turbomcp-wasm │
//!    │ (native impl) │               │ (WASM impl)   │
//!    │               │               │               │
//!    │ JwtValidator  │               │ WasmJwtAuth   │
//!    │ (jsonwebtoken)│               │ (Web Crypto)  │
//!    └───────────────┘               └───────────────┘
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use turbomcp_core::auth::{Authenticator, Credential, Principal};
//!
//! // Implement for your platform
//! struct MyAuthenticator;
//!
//! impl Authenticator for MyAuthenticator {
//!     type Error = MyError;
//!
//!     async fn authenticate(&self, credential: &Credential) -> Result<Principal, Self::Error> {
//!         // Validate credential and return principal
//!     }
//! }
//! ```

use crate::marker::{MaybeSend, MaybeSync};
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;
use core::future::Future;
use serde::{Deserialize, Serialize};

/// Credential types that can be extracted from requests.
///
/// This enum represents the various authentication credentials
/// that MCP servers may accept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credential {
    /// Bearer token (e.g., JWT, OAuth access token)
    /// Extracted from: `Authorization: Bearer <token>`
    Bearer(String),

    /// API key
    /// Extracted from: `X-API-Key: <key>` or `Authorization: ApiKey <key>`
    ApiKey(String),

    /// Basic authentication
    /// Extracted from: `Authorization: Basic <base64(username:password)>`
    Basic {
        /// Username
        username: String,
        /// Password
        password: String,
    },

    /// Custom credential scheme
    /// For platform-specific auth (e.g., Cloudflare Access)
    Custom {
        /// Scheme name (e.g., "CF-Access")
        scheme: String,
        /// Credential value
        value: String,
    },
}

impl Credential {
    /// Create a Bearer credential
    pub fn bearer(token: impl Into<String>) -> Self {
        Self::Bearer(token.into())
    }

    /// Create an API key credential
    pub fn api_key(key: impl Into<String>) -> Self {
        Self::ApiKey(key.into())
    }

    /// Create a Basic auth credential
    pub fn basic(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self::Basic {
            username: username.into(),
            password: password.into(),
        }
    }

    /// Create a custom credential
    pub fn custom(scheme: impl Into<String>, value: impl Into<String>) -> Self {
        Self::Custom {
            scheme: scheme.into(),
            value: value.into(),
        }
    }

    /// Returns true if this is a Bearer credential
    pub fn is_bearer(&self) -> bool {
        matches!(self, Self::Bearer(_))
    }

    /// Returns the bearer token if this is a Bearer credential
    pub fn as_bearer(&self) -> Option<&str> {
        match self {
            Self::Bearer(token) => Some(token),
            _ => None,
        }
    }
}

/// A validated principal (identity) after successful authentication.
///
/// This struct contains the authenticated identity and any claims
/// extracted from the credential (e.g., JWT claims).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Principal {
    /// Unique subject identifier (e.g., user ID, `sub` claim in JWT)
    pub subject: String,

    /// Token/credential issuer (e.g., `iss` claim in JWT)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,

    /// Intended audience (e.g., `aud` claim in JWT)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<String>,

    /// Expiration timestamp (Unix epoch seconds)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,

    /// Email address if available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// Display name if available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Roles/permissions granted
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<String>,

    /// Additional claims (platform-specific)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub claims: BTreeMap<String, serde_json::Value>,
}

impl Principal {
    /// Create a new principal with just a subject
    pub fn new(subject: impl Into<String>) -> Self {
        Self {
            subject: subject.into(),
            issuer: None,
            audience: None,
            expires_at: None,
            email: None,
            name: None,
            roles: Vec::new(),
            claims: BTreeMap::new(),
        }
    }

    /// Builder: set issuer
    pub fn with_issuer(mut self, issuer: impl Into<String>) -> Self {
        self.issuer = Some(issuer.into());
        self
    }

    /// Builder: set audience
    pub fn with_audience(mut self, audience: impl Into<String>) -> Self {
        self.audience = Some(audience.into());
        self
    }

    /// Builder: set expiration
    pub fn with_expires_at(mut self, expires_at: u64) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Builder: set email
    pub fn with_email(mut self, email: impl Into<String>) -> Self {
        self.email = Some(email.into());
        self
    }

    /// Builder: set name
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Builder: add a role
    pub fn with_role(mut self, role: impl Into<String>) -> Self {
        self.roles.push(role.into());
        self
    }

    /// Builder: add multiple roles
    pub fn with_roles(mut self, roles: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.roles.extend(roles.into_iter().map(Into::into));
        self
    }

    /// Builder: add a custom claim
    pub fn with_claim(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.claims.insert(key.into(), value);
        self
    }

    /// Check if principal has a specific role
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }

    /// Check if principal has any of the specified roles
    pub fn has_any_role(&self, roles: &[&str]) -> bool {
        roles.iter().any(|r| self.has_role(r))
    }

    /// Check if the principal is expired
    ///
    /// Note: In `no_std` environments, this always returns `false` since
    /// there's no standard way to get the current time. Use
    /// [`Principal::expires_at`] to check expiration manually.
    pub fn is_expired(&self) -> bool {
        #[cfg(feature = "std")]
        {
            if let Some(exp) = self.expires_at {
                use std::time::{SystemTime, UNIX_EPOCH};
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                now > exp
            } else {
                false
            }
        }
        #[cfg(not(feature = "std"))]
        {
            // In no_std, caller must check expiration externally
            // since there's no standard way to get current time
            let _ = self.expires_at;
            false
        }
    }

    /// Get a custom claim by key
    pub fn get_claim(&self, key: &str) -> Option<&serde_json::Value> {
        self.claims.get(key)
    }
}

impl fmt::Display for Principal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Principal({})", self.subject)
    }
}

/// Authentication error types.
#[derive(Debug, Clone)]
pub enum AuthError {
    /// No credentials provided
    MissingCredentials,

    /// Invalid credential format
    InvalidCredentialFormat(String),

    /// Credential type not supported
    UnsupportedCredentialType,

    /// Token has expired
    TokenExpired,

    /// Token signature is invalid
    InvalidSignature,

    /// Token claims validation failed
    InvalidClaims(String),

    /// Issuer mismatch
    InvalidIssuer {
        /// Expected issuer
        expected: String,
        /// Actual issuer received
        actual: String,
    },

    /// Audience mismatch
    InvalidAudience {
        /// Expected audience
        expected: String,
        /// Actual audience received
        actual: String,
    },

    /// Key not found (e.g., kid not in JWKS)
    KeyNotFound(String),

    /// Failed to fetch JWKS or other key material
    KeyFetchError(String),

    /// Internal error
    Internal(String),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCredentials => write!(f, "No credentials provided"),
            Self::InvalidCredentialFormat(msg) => write!(f, "Invalid credential format: {}", msg),
            Self::UnsupportedCredentialType => write!(f, "Unsupported credential type"),
            Self::TokenExpired => write!(f, "Token has expired"),
            Self::InvalidSignature => write!(f, "Invalid token signature"),
            Self::InvalidClaims(msg) => write!(f, "Invalid claims: {}", msg),
            Self::InvalidIssuer { expected, actual } => {
                write!(
                    f,
                    "Invalid issuer: expected '{}', got '{}'",
                    expected, actual
                )
            }
            Self::InvalidAudience { expected, actual } => {
                write!(
                    f,
                    "Invalid audience: expected '{}', got '{}'",
                    expected, actual
                )
            }
            Self::KeyNotFound(kid) => write!(f, "Key not found: {}", kid),
            Self::KeyFetchError(msg) => write!(f, "Failed to fetch keys: {}", msg),
            Self::Internal(msg) => write!(f, "Internal auth error: {}", msg),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for AuthError {}

/// Core authentication trait.
///
/// Implementors validate credentials and return an authenticated [`Principal`].
/// This trait is platform-adaptive using [`MaybeSend`] bounds.
///
/// # Platform Implementations
///
/// - **Native** (`turbomcp-auth`): Uses `jsonwebtoken` crate with tokio
/// - **WASM** (`turbomcp-wasm`): Uses Web Crypto API via `web-sys`
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_core::auth::{Authenticator, AuthError, Credential, Principal};
///
/// struct ApiKeyAuth {
///     valid_keys: Vec<String>,
/// }
///
/// impl Authenticator for ApiKeyAuth {
///     type Error = AuthError;
///
///     async fn authenticate(&self, credential: &Credential) -> Result<Principal, Self::Error> {
///         let key = credential.as_bearer()
///             .or_else(|| match credential {
///                 Credential::ApiKey(k) => Some(k.as_str()),
///                 _ => None,
///             })
///             .ok_or(AuthError::UnsupportedCredentialType)?;
///
///         if self.valid_keys.contains(&key.to_string()) {
///             Ok(Principal::new("api-user"))
///         } else {
///             Err(AuthError::InvalidSignature)
///         }
///     }
/// }
/// ```
pub trait Authenticator: MaybeSend + MaybeSync + Clone {
    /// Error type returned on authentication failure
    type Error: fmt::Debug + fmt::Display + MaybeSend;

    /// Validate a credential and return the authenticated principal.
    ///
    /// # Arguments
    ///
    /// * `credential` - The credential to validate
    ///
    /// # Returns
    ///
    /// * `Ok(Principal)` - Authentication successful
    /// * `Err(Self::Error)` - Authentication failed
    fn authenticate(
        &self,
        credential: &Credential,
    ) -> impl Future<Output = Result<Principal, Self::Error>> + MaybeSend;
}

/// Extracts credentials from request context.
///
/// Implementors extract authentication credentials from various sources
/// (headers, query params, cookies, etc.).
///
/// # Default Implementation
///
/// [`HeaderExtractor`] extracts from the `Authorization` header:
/// - `Bearer <token>` → [`Credential::Bearer`]
/// - `Basic <base64>` → [`Credential::Basic`]
/// - `ApiKey <key>` → [`Credential::ApiKey`]
pub trait CredentialExtractor: MaybeSend + MaybeSync {
    /// Extract credentials from the given headers.
    ///
    /// # Arguments
    ///
    /// * `get_header` - Function to retrieve header value by name
    ///
    /// # Returns
    ///
    /// * `Some(Credential)` - Credential found
    /// * `None` - No credential present
    fn extract<F>(&self, get_header: F) -> Option<Credential>
    where
        F: Fn(&str) -> Option<String>;
}

/// Default credential extractor that reads the Authorization header.
#[derive(Debug, Clone, Copy, Default)]
pub struct HeaderExtractor;

impl CredentialExtractor for HeaderExtractor {
    fn extract<F>(&self, get_header: F) -> Option<Credential>
    where
        F: Fn(&str) -> Option<String>,
    {
        // Try Authorization header first
        if let Some(auth) = get_header("authorization") {
            let auth = auth.trim();

            // Bearer token
            if let Some(token) = auth
                .strip_prefix("Bearer ")
                .or_else(|| auth.strip_prefix("bearer "))
            {
                return Some(Credential::Bearer(token.trim().to_string()));
            }

            // Basic auth - requires std feature for base64 decoding
            #[cfg(feature = "std")]
            if let Some(encoded) = auth
                .strip_prefix("Basic ")
                .or_else(|| auth.strip_prefix("basic "))
            {
                use base64::Engine;
                if let Ok(decoded) =
                    base64::engine::general_purpose::STANDARD.decode(encoded.trim())
                    && let Ok(decoded_str) = String::from_utf8(decoded)
                    && let Some((username, password)) = decoded_str.split_once(':')
                {
                    return Some(Credential::Basic {
                        username: username.to_string(),
                        password: password.to_string(),
                    });
                }
            }

            // ApiKey scheme
            if let Some(key) = auth
                .strip_prefix("ApiKey ")
                .or_else(|| auth.strip_prefix("apikey "))
            {
                return Some(Credential::ApiKey(key.trim().to_string()));
            }
        }

        // Try X-API-Key header
        if let Some(key) = get_header("x-api-key") {
            return Some(Credential::ApiKey(key.trim().to_string()));
        }

        None
    }
}

/// JWT validation configuration.
///
/// Platform-agnostic configuration for JWT validation.
///
/// # Security
///
/// **IMPORTANT**: Always use [`JwtConfig::new()`] to create a configuration with
/// secure defaults. The `Default` trait is intentionally NOT implemented to prevent
/// accidental creation of insecure configurations with empty algorithm whitelists.
///
/// ```rust
/// use turbomcp_core::auth::{JwtConfig, JwtAlgorithm};
///
/// // ✅ CORRECT: Use new() for secure defaults (RS256, ES256)
/// let config = JwtConfig::new()
///     .issuer("https://auth.example.com")
///     .audience("my-api");
///
/// // ✅ CORRECT: Explicitly specify algorithms
/// let config = JwtConfig::new()
///     .algorithms(vec![JwtAlgorithm::RS256]);
/// ```
#[derive(Debug, Clone)]
pub struct JwtConfig {
    /// Expected issuer (`iss` claim)
    pub issuer: Option<String>,

    /// Expected audience (`aud` claim)
    pub audience: Option<String>,

    /// Allowed signing algorithms.
    ///
    /// **Security**: This list MUST NOT be empty. An empty list will cause
    /// token validation to fail with an error. Always specify at least one
    /// algorithm explicitly.
    pub algorithms: Vec<JwtAlgorithm>,

    /// Clock skew tolerance in seconds (default: 60)
    pub leeway_seconds: u64,

    /// Whether to validate expiration (default: true)
    pub validate_exp: bool,

    /// Whether to validate not-before (default: true)
    pub validate_nbf: bool,
}

impl JwtConfig {
    /// Create a new JWT config with sensible defaults.
    ///
    /// # Security
    ///
    /// This method provides secure defaults:
    /// - `algorithms`: `[RS256, ES256]` (asymmetric algorithms only)
    /// - `validate_exp`: `true`
    /// - `validate_nbf`: `true`
    /// - `leeway_seconds`: `60`
    ///
    /// The `Default` trait is intentionally NOT implemented to prevent
    /// accidental creation of configurations with an empty algorithm list,
    /// which would bypass algorithm validation and enable algorithm
    /// confusion attacks.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            issuer: None,
            audience: None,
            algorithms: vec![JwtAlgorithm::RS256, JwtAlgorithm::ES256],
            leeway_seconds: 60,
            validate_exp: true,
            validate_nbf: true,
        }
    }

    /// Builder: set expected issuer
    pub fn issuer(mut self, issuer: impl Into<String>) -> Self {
        self.issuer = Some(issuer.into());
        self
    }

    /// Builder: set expected audience
    pub fn audience(mut self, audience: impl Into<String>) -> Self {
        self.audience = Some(audience.into());
        self
    }

    /// Builder: set allowed algorithms
    pub fn algorithms(mut self, algorithms: Vec<JwtAlgorithm>) -> Self {
        self.algorithms = algorithms;
        self
    }

    /// Builder: set clock skew leeway
    pub fn leeway_seconds(mut self, seconds: u64) -> Self {
        self.leeway_seconds = seconds;
        self
    }

    /// Builder: disable expiration validation
    pub fn skip_exp_validation(mut self) -> Self {
        self.validate_exp = false;
        self
    }

    /// Builder: disable not-before validation
    pub fn skip_nbf_validation(mut self) -> Self {
        self.validate_nbf = false;
        self
    }
}

/// JWT signing algorithms supported across platforms.
///
/// This is a subset of algorithms that work on both native and WASM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum JwtAlgorithm {
    /// HMAC with SHA-256
    HS256,
    /// HMAC with SHA-384
    HS384,
    /// HMAC with SHA-512
    HS512,
    /// RSA PKCS#1 with SHA-256
    RS256,
    /// RSA PKCS#1 with SHA-384
    RS384,
    /// RSA PKCS#1 with SHA-512
    RS512,
    /// ECDSA with P-256 and SHA-256
    ES256,
    /// ECDSA with P-384 and SHA-384
    ES384,
}

impl JwtAlgorithm {
    /// Returns the algorithm name as used in JWT headers
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::HS256 => "HS256",
            Self::HS384 => "HS384",
            Self::HS512 => "HS512",
            Self::RS256 => "RS256",
            Self::RS384 => "RS384",
            Self::RS512 => "RS512",
            Self::ES256 => "ES256",
            Self::ES384 => "ES384",
        }
    }

    /// Parse algorithm from string (convenience method)
    ///
    /// Returns `None` if the string is not a recognized algorithm.
    /// For infallible parsing, use `str::parse::<JwtAlgorithm>()`.
    pub fn parse(s: &str) -> Option<Self> {
        s.parse().ok()
    }

    /// Check if this is an asymmetric algorithm (RSA or ECDSA)
    pub fn is_asymmetric(&self) -> bool {
        matches!(
            self,
            Self::RS256 | Self::RS384 | Self::RS512 | Self::ES256 | Self::ES384
        )
    }

    /// Check if this is a symmetric algorithm (HMAC)
    pub fn is_symmetric(&self) -> bool {
        matches!(self, Self::HS256 | Self::HS384 | Self::HS512)
    }
}

impl fmt::Display for JwtAlgorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl core::str::FromStr for JwtAlgorithm {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "HS256" => Ok(Self::HS256),
            "HS384" => Ok(Self::HS384),
            "HS512" => Ok(Self::HS512),
            "RS256" => Ok(Self::RS256),
            "RS384" => Ok(Self::RS384),
            "RS512" => Ok(Self::RS512),
            "ES256" => Ok(Self::ES256),
            "ES384" => Ok(Self::ES384),
            _ => Err(()),
        }
    }
}

/// Standard JWT claims.
///
/// These are the registered claims from RFC 7519.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StandardClaims {
    /// Subject (user identifier)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,

    /// Issuer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,

    /// Audience (can be string or array)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aud: Option<Audience>,

    /// Expiration time (Unix timestamp)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<u64>,

    /// Not before time (Unix timestamp)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nbf: Option<u64>,

    /// Issued at time (Unix timestamp)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<u64>,

    /// JWT ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,
}

/// JWT audience claim (can be a single string or array of strings).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Audience {
    /// Single audience
    Single(String),
    /// Multiple audiences
    Multiple(Vec<String>),
}

impl Audience {
    /// Check if the audience contains a specific value
    pub fn contains(&self, expected: &str) -> bool {
        match self {
            Self::Single(s) => s == expected,
            Self::Multiple(v) => v.iter().any(|s| s == expected),
        }
    }

    /// Convert to a vector of strings
    pub fn to_vec(&self) -> Vec<String> {
        match self {
            Self::Single(s) => vec![s.clone()],
            Self::Multiple(v) => v.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credential_constructors() {
        let bearer = Credential::bearer("token123");
        assert!(bearer.is_bearer());
        assert_eq!(bearer.as_bearer(), Some("token123"));

        let api_key = Credential::api_key("key456");
        assert!(!api_key.is_bearer());
        assert_eq!(api_key.as_bearer(), None);

        let basic = Credential::basic("user", "pass");
        assert!(!basic.is_bearer());
    }

    #[test]
    fn test_principal_builder() {
        let principal = Principal::new("user123")
            .with_issuer("https://auth.example.com")
            .with_audience("my-api")
            .with_email("user@example.com")
            .with_role("admin")
            .with_role("user");

        assert_eq!(principal.subject, "user123");
        assert_eq!(
            principal.issuer,
            Some("https://auth.example.com".to_string())
        );
        assert!(principal.has_role("admin"));
        assert!(principal.has_role("user"));
        assert!(!principal.has_role("guest"));
        assert!(principal.has_any_role(&["admin", "guest"]));
    }

    #[test]
    fn test_header_extractor_bearer() {
        let extractor = HeaderExtractor;

        let cred = extractor.extract(|name| {
            if name == "authorization" {
                Some("Bearer my-token".to_string())
            } else {
                None
            }
        });

        assert_eq!(cred, Some(Credential::Bearer("my-token".to_string())));
    }

    #[test]
    fn test_header_extractor_api_key() {
        let extractor = HeaderExtractor;

        // Via X-API-Key header
        let cred = extractor.extract(|name| {
            if name == "x-api-key" {
                Some("my-api-key".to_string())
            } else {
                None
            }
        });

        assert_eq!(cred, Some(Credential::ApiKey("my-api-key".to_string())));

        // Via Authorization: ApiKey
        let cred2 = extractor.extract(|name| {
            if name == "authorization" {
                Some("ApiKey another-key".to_string())
            } else {
                None
            }
        });

        assert_eq!(cred2, Some(Credential::ApiKey("another-key".to_string())));
    }

    #[test]
    fn test_jwt_algorithm() {
        assert_eq!(JwtAlgorithm::RS256.as_str(), "RS256");
        assert!(JwtAlgorithm::RS256.is_asymmetric());
        assert!(!JwtAlgorithm::RS256.is_symmetric());

        assert!(JwtAlgorithm::HS256.is_symmetric());
        assert!(!JwtAlgorithm::HS256.is_asymmetric());

        assert_eq!(JwtAlgorithm::parse("es256"), Some(JwtAlgorithm::ES256));
        assert_eq!(JwtAlgorithm::parse("unknown"), None);
    }

    #[test]
    fn test_audience() {
        let single = Audience::Single("my-api".to_string());
        assert!(single.contains("my-api"));
        assert!(!single.contains("other"));

        let multiple = Audience::Multiple(vec!["api1".to_string(), "api2".to_string()]);
        assert!(multiple.contains("api1"));
        assert!(multiple.contains("api2"));
        assert!(!multiple.contains("api3"));
    }

    #[test]
    fn test_jwt_config_builder() {
        let config = JwtConfig::new()
            .issuer("https://auth.example.com")
            .audience("my-api")
            .algorithms(vec![JwtAlgorithm::RS256])
            .leeway_seconds(120);

        assert_eq!(config.issuer, Some("https://auth.example.com".to_string()));
        assert_eq!(config.audience, Some("my-api".to_string()));
        assert_eq!(config.algorithms, vec![JwtAlgorithm::RS256]);
        assert_eq!(config.leeway_seconds, 120);
    }
}
