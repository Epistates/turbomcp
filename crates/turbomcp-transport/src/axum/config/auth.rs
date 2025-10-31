//! Authentication configuration management
//!
//! This module provides authentication configuration for various
//! authentication methods including JWT and API keys.

/// JWT algorithm for token validation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JwtAlgorithm {
    /// HMAC using SHA-256 (symmetric)
    HS256,
    /// HMAC using SHA-384 (symmetric)
    HS384,
    /// HMAC using SHA-512 (symmetric)
    HS512,
    /// RSASSA-PKCS1-v1_5 using SHA-256 (asymmetric)
    RS256,
    /// RSASSA-PKCS1-v1_5 using SHA-384 (asymmetric)
    RS384,
    /// RSASSA-PKCS1-v1_5 using SHA-512 (asymmetric)
    RS512,
    /// ECDSA using P-256 and SHA-256 (asymmetric)
    ES256,
    /// ECDSA using P-384 and SHA-384 (asymmetric)
    ES384,
}

impl Default for JwtAlgorithm {
    fn default() -> Self {
        Self::HS256
    }
}

/// JWT validation configuration
#[derive(Debug, Clone)]
pub struct JwtConfig {
    /// JWT secret for HS256/HS384/HS512 (symmetric algorithms)
    pub secret: Option<String>,
    /// JWKS URI for RS256/ES256 (asymmetric algorithms)
    pub jwks_uri: Option<String>,
    /// Algorithm to use for validation
    pub algorithm: JwtAlgorithm,
    /// Required audience (aud claim)
    pub audience: Option<Vec<String>>,
    /// Required issuer (iss claim)
    pub issuer: Option<Vec<String>>,
    /// Validate expiration (exp claim)
    pub validate_exp: bool,
    /// Validate not before (nbf claim)
    pub validate_nbf: bool,
    /// Leeway in seconds for time-based validations
    pub leeway: u64,

    /// Server's canonical URI for audience validation (RFC 8707)
    ///
    /// If set, the middleware will validate that the JWT's `aud` claim matches
    /// this URI using RFC 8707 normalization rules.
    ///
    /// Example: "https://api.example.com" or "https://api.example.com/mcp"
    pub server_uri: Option<String>,

    /// Token introspection endpoint (RFC 7662)
    ///
    /// If set, tokens will be validated via introspection in addition to JWT signature.
    /// This enables real-time revocation checking.
    ///
    /// Example: "https://auth.example.com/oauth/introspect"
    pub introspection_endpoint: Option<String>,

    /// Client ID for introspection requests
    pub introspection_client_id: Option<String>,

    /// Client secret for introspection requests
    pub introspection_client_secret: Option<String>,
}

impl Default for JwtConfig {
    fn default() -> Self {
        Self {
            secret: None,
            jwks_uri: None,
            algorithm: JwtAlgorithm::default(),
            audience: None,
            issuer: None,
            validate_exp: true,
            validate_nbf: true,
            leeway: 60, // 60 seconds leeway for clock skew
            server_uri: None,
            introspection_endpoint: None,
            introspection_client_id: None,
            introspection_client_secret: None,
        }
    }
}

/// Authentication configuration for middleware
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Enable authentication
    pub enabled: bool,
    /// JWT configuration for token validation
    pub jwt: Option<JwtConfig>,
    /// API key header name
    pub api_key_header: Option<String>,
    /// Custom authentication provider
    pub custom_validator: Option<String>,

    /// Protected Resource Metadata URI for WWW-Authenticate header (RFC 9728)
    ///
    /// Per MCP spec, servers MUST return this in WWW-Authenticate header on 401.
    /// Example: "https://api.example.com/.well-known/oauth-protected-resource"
    pub resource_metadata_uri: Option<String>,

    /// Required scopes for WWW-Authenticate header
    pub required_scopes: Vec<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            jwt: None,
            api_key_header: Some("x-api-key".to_string()),
            custom_validator: None,
            resource_metadata_uri: None,
            required_scopes: vec![],
        }
    }
}

impl AuthConfig {
    /// Create new authentication config with JWT (HS256)
    pub fn jwt(secret: String) -> Self {
        Self {
            enabled: true,
            jwt: Some(JwtConfig {
                secret: Some(secret),
                ..Default::default()
            }),
            api_key_header: None,
            custom_validator: None,
            resource_metadata_uri: None,
            required_scopes: vec![],
        }
    }

    /// Create new authentication config with JWT and full options
    pub fn jwt_with_config(jwt_config: JwtConfig) -> Self {
        Self {
            enabled: true,
            jwt: Some(jwt_config),
            api_key_header: None,
            custom_validator: None,
            resource_metadata_uri: None,
            required_scopes: vec![],
        }
    }

    /// Create new authentication config with API key
    pub fn api_key(header: String) -> Self {
        Self {
            enabled: true,
            jwt: None,
            api_key_header: Some(header),
            custom_validator: None,
            resource_metadata_uri: None,
            required_scopes: vec![],
        }
    }

    /// Create new authentication config with custom validator
    pub fn custom(validator: String) -> Self {
        Self {
            enabled: true,
            jwt: None,
            api_key_header: None,
            custom_validator: Some(validator),
            resource_metadata_uri: None,
            required_scopes: vec![],
        }
    }

    /// Disable authentication
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            jwt: None,
            api_key_header: None,
            custom_validator: None,
            resource_metadata_uri: None,
            required_scopes: vec![],
        }
    }

    /// Set the protected resource metadata URI (for WWW-Authenticate header)
    ///
    /// # MCP Specification Compliance
    ///
    /// Per MCP spec (RFC 9728), servers MUST return WWW-Authenticate header
    /// on 401 responses with the location of the metadata endpoint.
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_transport::axum::AuthConfig;
    ///
    /// let config = AuthConfig::jwt("secret".to_string())
    ///     .with_metadata_uri("https://api.example.com/.well-known/oauth-protected-resource");
    /// ```
    pub fn with_metadata_uri(mut self, uri: impl Into<String>) -> Self {
        self.resource_metadata_uri = Some(uri.into());
        self
    }

    /// Set required scopes (for WWW-Authenticate header)
    pub fn with_required_scopes(mut self, scopes: Vec<String>) -> Self {
        self.required_scopes = scopes;
        self
    }
}
