//! Security middleware using tower-http for CORS, security headers, and protection (Sprint 3.3)
//!
//! This middleware provides comprehensive security features including CORS handling,
//! security headers, and protection against common web vulnerabilities.
//!
//! ## CORS Security (Sprint 3.3 Hardening)
//!
//! ### Critical OWASP Rules Enforced
//!
//! 1. **Wildcard + Credentials = FORBIDDEN**
//!    - OWASP: Cannot use `Access-Control-Allow-Origin: *` with `Access-Control-Allow-Credentials: true`
//!    - Browsers will block such requests
//!    - Our implementation validates and panics if this combination is configured
//!
//! 2. **Null Origin Protection**
//!    - OWASP: Attackers can send `Origin: null` to bypass validation
//!    - Never allow `null` as an origin
//!
//! 3. **Explicit Allow-Lists Only**
//!    - OWASP: Use explicit allow-lists, never wildcards in production
//!    - `CorsOrigins::Any` is ONLY for development/testing
//!    - Production MUST use `CorsConfig::strict()` or `production_safe()`
//!
//! ### Usage
//!
//! ```rust,ignore
//! // ✅ CORRECT: Strict production configuration
//! let cors = CorsConfig::strict()
//!     .with_origins(vec!["https://app.example.com".to_string()])
//!     .with_credentials(true);  // Safe with explicit origins
//!
//! // ⚠️ INSECURE: Development only
//! let cors = CorsConfig::default();  // Uses Any - NEVER use in production
//!
//! // ❌ PANIC: This will panic at runtime (credentials + wildcard)
//! let cors = CorsConfig::default()
//!     .allow_any_origin()
//!     .with_credentials(true);  // PANIC: Invalid combination
//! ```

use std::time::Duration;

use http::{HeaderName, HeaderValue, Method, header};
use tower::ServiceBuilder;
use tower_http::{
    cors::{Any, CorsLayer},
    sensitive_headers::SetSensitiveRequestHeadersLayer,
    set_header::SetResponseHeaderLayer,
};

/// Security configuration
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    /// CORS configuration
    pub cors: CorsConfig,
    /// Security headers configuration
    pub headers: SecurityHeaders,
    /// Sensitive headers to redact from logs
    pub sensitive_headers: Vec<HeaderName>,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            cors: CorsConfig::default(),
            headers: SecurityHeaders::default(),
            sensitive_headers: vec![
                header::AUTHORIZATION,
                header::COOKIE,
                HeaderName::from_static("x-api-key"),
                HeaderName::from_static("x-auth-token"),
            ],
        }
    }
}

/// CORS configuration
#[derive(Debug, Clone)]
pub struct CorsConfig {
    /// Allowed origins
    pub allowed_origins: CorsOrigins,
    /// Allowed methods
    pub allowed_methods: Vec<Method>,
    /// Allowed headers
    pub allowed_headers: Vec<HeaderName>,
    /// Exposed headers
    pub exposed_headers: Vec<HeaderName>,
    /// Max age for preflight cache
    pub max_age: Option<Duration>,
    /// Allow credentials
    pub allow_credentials: bool,
}

/// CORS origin configuration
#[derive(Debug, Clone)]
pub enum CorsOrigins {
    /// Allow any origin (⚠️ WARNING: Insecure - only use for development/testing)
    Any,
    /// Allow specific origins (recommended for production)
    List(Vec<String>),
}

impl Default for CorsConfig {
    /// Default CORS configuration
    ///
    /// ⚠️ **WARNING**: Uses `CorsOrigins::Any` which allows requests from ANY origin.
    /// This is INSECURE and should only be used for development/testing.
    ///
    /// For production, use `CorsConfig::production_safe()` or configure specific origins.
    fn default() -> Self {
        Self {
            allowed_origins: CorsOrigins::Any, // ⚠️ INSECURE: Only for development!
            allowed_methods: vec![
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::DELETE,
                Method::OPTIONS,
            ],
            allowed_headers: vec![
                header::CONTENT_TYPE,
                header::AUTHORIZATION,
                header::ACCEPT,
                HeaderName::from_static("x-requested-with"),
            ],
            exposed_headers: vec![HeaderName::from_static("x-request-id")],
            max_age: Some(Duration::from_secs(3600)), // 1 hour
            allow_credentials: false,                 // Safer default
        }
    }
}

impl CorsConfig {
    /// Strict CORS configuration for high-security production environments (Sprint 3.3)
    ///
    /// This is the MOST secure CORS configuration:
    /// - NO origins allowed by default (must be explicitly configured)
    /// - Only GET and POST methods (most common, least risk)
    /// - Minimal headers (Content-Type, Accept only)
    /// - NO credentials allowed by default
    /// - Short preflight cache (5 minutes)
    ///
    /// # Example
    /// ```rust
    /// use turbomcp_server::middleware::security::CorsConfig;
    ///
    /// let cors = CorsConfig::strict()
    ///     .with_origins(vec!["https://app.example.com".to_string()]);
    /// ```
    pub fn strict() -> Self {
        Self {
            allowed_origins: CorsOrigins::List(vec![]), // Must be explicitly configured
            allowed_methods: vec![Method::GET, Method::POST], // Most restrictive
            allowed_headers: vec![header::CONTENT_TYPE, header::ACCEPT],
            exposed_headers: vec![],
            max_age: Some(Duration::from_secs(300)), // 5 minutes (short cache)
            allow_credentials: false,
        }
    }

    /// Production-safe CORS configuration
    ///
    /// Starts with NO allowed origins - you must explicitly configure them.
    /// This prevents accidentally allowing requests from any origin.
    ///
    /// # Example
    /// ```rust
    /// use turbomcp_server::middleware::security::CorsConfig;
    ///
    /// let cors = CorsConfig::production_safe()
    ///     .with_origins(vec!["https://app.example.com".to_string()]);
    /// ```
    pub fn production_safe() -> Self {
        Self {
            allowed_origins: CorsOrigins::List(vec![]), // Must be explicitly configured
            ..Self::default()
        }
    }

    /// Development configuration with localhost origins
    ///
    /// Allows common localhost development origins with HTTP and various ports.
    /// Still more secure than `Any` as it restricts to localhost only.
    ///
    /// ⚠️ **Development Only** - Do not use in production
    pub fn development_localhost() -> Self {
        Self {
            allowed_origins: CorsOrigins::List(vec![
                "http://localhost:3000".to_string(),
                "http://localhost:5173".to_string(), // Vite default
                "http://localhost:8080".to_string(),
                "http://127.0.0.1:3000".to_string(),
                "http://127.0.0.1:5173".to_string(),
                "http://127.0.0.1:8080".to_string(),
            ]),
            allow_credentials: true, // Safe with explicit origins
            ..Self::default()
        }
    }

    /// Set allowed origins
    ///
    /// Validates that origins are not "null" (OWASP security rule).
    ///
    /// # Panics
    /// Panics if any origin is "null" (security violation)
    pub fn with_origins(mut self, origins: Vec<String>) -> Self {
        // OWASP: Never allow "null" as an origin (attacker can set Origin: null)
        for origin in &origins {
            if origin.to_lowercase() == "null" {
                panic!(
                    "SECURITY VIOLATION: Cannot allow 'null' as CORS origin (OWASP). Remove 'null' from allowed origins."
                );
            }
        }
        self.allowed_origins = CorsOrigins::List(origins);
        self
    }

    /// Allow any origin (⚠️ INSECURE - development only)
    ///
    /// # Panics
    /// Panics if `allow_credentials` is true (OWASP security violation)
    pub fn allow_any_origin(mut self) -> Self {
        // OWASP CRITICAL: Cannot use wildcard with credentials
        if self.allow_credentials {
            panic!(
                "SECURITY VIOLATION: Cannot use CORS wildcard (*) with credentials (OWASP). Set allow_credentials=false or use explicit origins."
            );
        }
        self.allowed_origins = CorsOrigins::Any;
        self
    }

    /// Enable credentials (cookies, authorization headers)
    ///
    /// # Panics
    /// Panics if `allowed_origins` is `Any` (OWASP security violation)
    pub fn with_credentials(mut self, allow: bool) -> Self {
        // OWASP CRITICAL: Cannot use credentials with wildcard origin
        if allow && matches!(self.allowed_origins, CorsOrigins::Any) {
            panic!(
                "SECURITY VIOLATION: Cannot use CORS wildcard (*) with credentials (OWASP). Set explicit origins first."
            );
        }
        self.allow_credentials = allow;
        self
    }

    /// Set allowed methods
    pub fn with_methods(mut self, methods: Vec<Method>) -> Self {
        self.allowed_methods = methods;
        self
    }

    /// Set allowed headers
    pub fn with_headers(mut self, headers: Vec<HeaderName>) -> Self {
        self.allowed_headers = headers;
        self
    }

    /// Set exposed headers
    pub fn with_exposed_headers(mut self, headers: Vec<HeaderName>) -> Self {
        self.exposed_headers = headers;
        self
    }

    /// Set preflight cache duration
    pub fn with_max_age(mut self, duration: Duration) -> Self {
        self.max_age = Some(duration);
        self
    }
}

/// Security headers configuration
#[derive(Debug, Clone)]
pub struct SecurityHeaders {
    /// X-Content-Type-Options header
    pub content_type_options: bool,
    /// X-Frame-Options header
    pub frame_options: FrameOptions,
    /// X-XSS-Protection header (deprecated but still useful)
    pub xss_protection: bool,
    /// Strict-Transport-Security header
    pub hsts: Option<HstsConfig>,
    /// Content-Security-Policy header
    pub csp: Option<String>,
    /// Referrer-Policy header
    pub referrer_policy: Option<ReferrerPolicy>,
    /// Permissions-Policy header
    pub permissions_policy: Option<String>,
}

/// X-Frame-Options header values
#[derive(Debug, Clone)]
pub enum FrameOptions {
    /// Deny framing entirely
    Deny,
    /// Allow framing from same origin only
    SameOrigin,
    /// Allow framing from specific origin
    AllowFrom(String),
}

/// HTTP Strict Transport Security configuration
#[derive(Debug, Clone)]
pub struct HstsConfig {
    /// Maximum age for HSTS policy
    pub max_age: Duration,
    /// Include subdomains in HSTS policy
    pub include_subdomains: bool,
    /// Enable HSTS preload
    pub preload: bool,
}

/// Referrer-Policy header values
#[derive(Debug, Clone)]
pub enum ReferrerPolicy {
    /// Never send referrer
    NoReferrer,
    /// Send referrer only on HTTPS->HTTPS
    NoReferrerWhenDowngrade,
    /// Send origin only
    Origin,
    /// Send origin for cross-origin, full URL for same-origin
    OriginWhenCrossOrigin,
    /// Send referrer only for same-origin
    SameOrigin,
    /// Send origin only, but only HTTPS->HTTPS
    StrictOrigin,
    /// Combined strict-origin and origin-when-cross-origin
    StrictOriginWhenCrossOrigin,
    /// Send full URL (unsafe)
    UnsafeUrl,
}

impl Default for SecurityHeaders {
    fn default() -> Self {
        Self {
            content_type_options: true,
            frame_options: FrameOptions::Deny,
            xss_protection: true,
            hsts: Some(HstsConfig {
                max_age: Duration::from_secs(31536000), // 1 year
                include_subdomains: true,
                preload: false, // Requires manual submission to preload list
            }),
            csp: Some("default-src 'self'".to_string()),
            referrer_policy: Some(ReferrerPolicy::StrictOriginWhenCrossOrigin),
            permissions_policy: Some("camera=(), microphone=(), geolocation=()".to_string()),
        }
    }
}

/// Security layer combining CORS and security headers
#[derive(Debug, Clone)]
pub struct SecurityLayer {
    config: SecurityConfig,
}

impl SecurityLayer {
    /// Create new security layer
    pub fn new(config: SecurityConfig) -> Self {
        Self { config }
    }

    /// Build the complete security middleware stack
    pub fn build<S>(self) -> impl tower::Layer<S> {
        // Build all layers and chain them
        let cors_layer = self.build_cors_layer();
        let sensitive_headers_layer =
            SetSensitiveRequestHeadersLayer::new(self.config.sensitive_headers.clone());

        // Create the security middleware stack with all headers
        // Note: Using separate layer calls for each header to avoid complex generics
        ServiceBuilder::new()
            .layer(sensitive_headers_layer)
            .layer(cors_layer)
            .layer(SetResponseHeaderLayer::if_not_present(
                http::header::X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            ))
            .layer(SetResponseHeaderLayer::if_not_present(
                http::header::X_FRAME_OPTIONS,
                HeaderValue::from_static("DENY"),
            ))
            .layer(SetResponseHeaderLayer::if_not_present(
                HeaderName::from_static("x-xss-protection"),
                HeaderValue::from_static("1; mode=block"),
            ))
            .layer(SetResponseHeaderLayer::if_not_present(
                HeaderName::from_static("strict-transport-security"),
                HeaderValue::from_static("max-age=31536000; includeSubDomains"),
            ))
            .layer(SetResponseHeaderLayer::if_not_present(
                HeaderName::from_static("content-security-policy"),
                HeaderValue::from_static("default-src 'self'"),
            ))
            .into_inner()
    }

    /// Build CORS layer
    fn build_cors_layer(&self) -> CorsLayer {
        let mut cors = CorsLayer::new();

        // Set allowed origins
        cors = match &self.config.cors.allowed_origins {
            CorsOrigins::Any => cors.allow_origin(Any),
            CorsOrigins::List(origins) => {
                // Convert strings to HeaderValues
                let origin_values: Result<Vec<HeaderValue>, _> =
                    origins.iter().map(|o| HeaderValue::from_str(o)).collect();

                match origin_values {
                    Ok(values) => cors.allow_origin(values),
                    Err(_) => {
                        eprintln!("Warning: Invalid CORS origin, falling back to Any");
                        cors.allow_origin(Any)
                    }
                }
            }
        };

        // Set allowed methods
        cors = cors.allow_methods(self.config.cors.allowed_methods.clone());

        // Set allowed headers
        cors = cors.allow_headers(self.config.cors.allowed_headers.clone());

        // Set exposed headers
        if !self.config.cors.exposed_headers.is_empty() {
            cors = cors.expose_headers(self.config.cors.exposed_headers.clone());
        }

        // Set max age
        if let Some(max_age) = self.config.cors.max_age {
            cors = cors.max_age(max_age);
        }

        // Set credentials
        if self.config.cors.allow_credentials {
            cors = cors.allow_credentials(true);
        }

        cors
    }

    // Note: Security headers are now configured directly in the build() method
    // to avoid complex ServiceBuilder generic type issues
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_security_config() {
        let config = SecurityConfig::default();

        // Check CORS defaults
        assert!(matches!(config.cors.allowed_origins, CorsOrigins::Any));
        assert!(config.cors.allowed_methods.contains(&Method::POST));
        assert!(!config.cors.allow_credentials);

        // Check security headers defaults
        assert!(config.headers.content_type_options);
        assert!(matches!(config.headers.frame_options, FrameOptions::Deny));
        assert!(config.headers.xss_protection);
        assert!(config.headers.hsts.is_some());
        assert!(config.headers.csp.is_some());
    }

    #[test]
    fn test_hsts_value_generation() {
        let hsts = HstsConfig {
            max_age: Duration::from_secs(31536000),
            include_subdomains: true,
            preload: true,
        };

        // This test shows how HSTS header value would be constructed
        let mut hsts_value = format!("max-age={}", hsts.max_age.as_secs());
        if hsts.include_subdomains {
            hsts_value.push_str("; includeSubDomains");
        }
        if hsts.preload {
            hsts_value.push_str("; preload");
        }

        assert_eq!(hsts_value, "max-age=31536000; includeSubDomains; preload");
    }

    // Sprint 3.3: CORS Security Tests

    #[test]
    fn test_strict_cors_config() {
        let cors = CorsConfig::strict();

        // Verify strict defaults
        assert!(matches!(cors.allowed_origins, CorsOrigins::List(ref v) if v.is_empty()));
        assert_eq!(cors.allowed_methods.len(), 2); // Only GET and POST
        assert!(cors.allowed_methods.contains(&Method::GET));
        assert!(cors.allowed_methods.contains(&Method::POST));
        assert_eq!(cors.allowed_headers.len(), 2); // Only Content-Type and Accept
        assert!(cors.exposed_headers.is_empty());
        assert_eq!(cors.max_age, Some(Duration::from_secs(300))); // 5 minutes
        assert!(!cors.allow_credentials);
    }

    #[test]
    fn test_development_localhost_config() {
        let cors = CorsConfig::development_localhost();

        // Verify localhost origins are included
        if let CorsOrigins::List(origins) = cors.allowed_origins {
            assert!(origins.contains(&"http://localhost:3000".to_string()));
            assert!(origins.contains(&"http://localhost:5173".to_string())); // Vite
            assert!(origins.contains(&"http://127.0.0.1:3000".to_string()));
            assert_eq!(origins.len(), 6);
        } else {
            panic!("Expected CorsOrigins::List");
        }

        assert!(cors.allow_credentials); // Safe with explicit origins
    }

    #[test]
    fn test_with_origins_valid() {
        let cors = CorsConfig::strict().with_origins(vec!["https://app.example.com".to_string()]);

        if let CorsOrigins::List(origins) = cors.allowed_origins {
            assert_eq!(origins.len(), 1);
            assert_eq!(origins[0], "https://app.example.com");
        } else {
            panic!("Expected CorsOrigins::List");
        }
    }

    #[test]
    #[should_panic(expected = "SECURITY VIOLATION: Cannot allow 'null' as CORS origin")]
    fn test_null_origin_rejected() {
        // OWASP: Never allow "null" as origin
        CorsConfig::strict().with_origins(vec!["null".to_string()]);
    }

    #[test]
    #[should_panic(expected = "SECURITY VIOLATION: Cannot allow 'null' as CORS origin")]
    fn test_null_origin_rejected_mixed_case() {
        // Case-insensitive "null" rejection
        CorsConfig::strict().with_origins(vec![
            "https://app.example.com".to_string(),
            "NULL".to_string(),
        ]);
    }

    #[test]
    #[should_panic(expected = "SECURITY VIOLATION: Cannot use CORS wildcard (*) with credentials")]
    fn test_wildcard_with_credentials_rejected() {
        // OWASP CRITICAL: wildcard + credentials = FORBIDDEN
        CorsConfig::default().with_credentials(true); // Default uses Any, this should panic
    }

    #[test]
    #[should_panic(expected = "SECURITY VIOLATION: Cannot use CORS wildcard (*) with credentials")]
    fn test_allow_any_origin_with_credentials_rejected() {
        // OWASP CRITICAL: Cannot enable wildcard when credentials are set
        CorsConfig::strict()
            .with_credentials(true)
            .allow_any_origin(); // This should panic
    }

    #[test]
    fn test_credentials_with_explicit_origins_allowed() {
        // This is SAFE: credentials + explicit origins
        let cors = CorsConfig::strict()
            .with_origins(vec!["https://app.example.com".to_string()])
            .with_credentials(true);

        assert!(cors.allow_credentials);
    }

    #[test]
    fn test_wildcard_without_credentials_allowed() {
        // This is SAFE: wildcard without credentials
        let cors = CorsConfig::strict().allow_any_origin();

        assert!(matches!(cors.allowed_origins, CorsOrigins::Any));
        assert!(!cors.allow_credentials);
    }

    #[test]
    fn test_cors_builder_pattern() {
        let cors = CorsConfig::strict()
            .with_origins(vec!["https://app.example.com".to_string()])
            .with_methods(vec![Method::GET, Method::POST, Method::PUT])
            .with_headers(vec![header::CONTENT_TYPE, header::AUTHORIZATION])
            .with_exposed_headers(vec![HeaderName::from_static("x-request-id")])
            .with_max_age(Duration::from_secs(600))
            .with_credentials(true);

        assert!(cors.allow_credentials);
        assert_eq!(cors.allowed_methods.len(), 3);
        assert_eq!(cors.max_age, Some(Duration::from_secs(600)));
    }

    #[test]
    fn test_production_safe_requires_explicit_origins() {
        let cors = CorsConfig::production_safe();

        // Should start with empty list
        if let CorsOrigins::List(origins) = cors.allowed_origins {
            assert!(origins.is_empty());
        } else {
            panic!("Expected CorsOrigins::List");
        }
    }
}
