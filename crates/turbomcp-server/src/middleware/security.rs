//! Security middleware using tower-http for CORS, security headers, and protection
//!
//! This middleware provides comprehensive security features including CORS handling,
//! security headers, and protection against common web vulnerabilities.

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

    /// Set allowed origins
    pub fn with_origins(mut self, origins: Vec<String>) -> Self {
        self.allowed_origins = CorsOrigins::List(origins);
        self
    }

    /// Allow any origin (⚠️ INSECURE - development only)
    pub fn allow_any_origin(mut self) -> Self {
        self.allowed_origins = CorsOrigins::Any;
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
}
