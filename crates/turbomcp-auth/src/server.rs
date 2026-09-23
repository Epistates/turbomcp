//! Server-side authentication and authorization helpers
//!
//! This module provides utilities for MCP servers to handle:
//! - Protected Resource Metadata discovery (RFC 9728)
//! - WWW-Authenticate header generation for 401 responses
//! - Token validation middleware helpers

use std::collections::HashMap;

use serde_json::{Value, json};
use turbomcp_protocol::{Error as McpError, Result as McpResult};

use crate::config::{BearerTokenMethod, ProtectedResourceMetadata};

/// Protected Resource Metadata endpoint builder
///
/// Helps construct RFC 9728 compliant Protected Resource Metadata responses
/// for the `/.well-known/oauth-protected-resource` endpoint (see
/// [`ProtectedResourceMetadata`] for the document type this builds). Both
/// [`Self::build`] (JSON) and [`Self::build_struct`] (typed) serialize the
/// same shape, matching RFC 9728 §2 field-for-field — in particular
/// `authorization_servers` as an array, even for the single-AS case
/// [`Self::new`] covers.
#[derive(Debug, Clone)]
pub struct ProtectedResourceMetadataBuilder {
    /// Base resource URI
    base_resource_uri: String,
    /// Authorization servers that can issue tokens for this resource
    auth_servers: Vec<String>,
    /// Supported scopes
    scopes: Vec<String>,
    /// Bearer token methods
    bearer_methods: Vec<BearerTokenMethod>,
    /// Resource documentation
    documentation_uri: Option<String>,
}

impl ProtectedResourceMetadataBuilder {
    /// Create a new metadata builder for the common single-authorization-server case.
    ///
    /// Use [`Self::with_additional_authorization_server`] to advertise more
    /// than one authorization server (RFC 9728 §2 allows an array; clients
    /// pick one per §7.6).
    pub fn new(base_resource_uri: String, auth_server: String) -> Self {
        Self {
            base_resource_uri,
            auth_servers: vec![auth_server],
            scopes: vec!["openid".to_string(), "profile".to_string()],
            bearer_methods: vec![BearerTokenMethod::Header, BearerTokenMethod::Body],
            documentation_uri: None,
        }
    }

    /// Advertise an additional authorization server for this resource.
    pub fn with_additional_authorization_server(mut self, auth_server: String) -> Self {
        self.auth_servers.push(auth_server);
        self
    }

    /// Set supported scopes
    pub fn with_scopes(mut self, scopes: Vec<String>) -> Self {
        self.scopes = scopes;
        self
    }

    /// Set bearer token methods
    pub fn with_bearer_methods(mut self, methods: Vec<BearerTokenMethod>) -> Self {
        self.bearer_methods = methods;
        self
    }

    /// Set documentation URI
    pub fn with_documentation(mut self, uri: String) -> Self {
        self.documentation_uri = Some(uri);
        self
    }

    /// Build the metadata as JSON value
    pub fn build(self) -> Value {
        let mut metadata = json!({
            "resource": self.base_resource_uri,
            "authorization_servers": self.auth_servers,
            "scopes_supported": self.scopes,
            "bearer_methods_supported": self.bearer_methods
                .iter()
                .map(|m| match m {
                    BearerTokenMethod::Header => "header",
                    BearerTokenMethod::Query => "query",
                    BearerTokenMethod::Body => "body",
                })
                .collect::<Vec<_>>(),
        });

        if let Some(doc) = self.documentation_uri {
            metadata["resource_documentation"] = Value::String(doc);
        }

        metadata
    }

    /// Build as a ProtectedResourceMetadata struct
    pub fn build_struct(self) -> ProtectedResourceMetadata {
        ProtectedResourceMetadata {
            resource: self.base_resource_uri,
            authorization_servers: self.auth_servers,
            scopes_supported: Some(self.scopes),
            bearer_methods_supported: Some(self.bearer_methods),
            resource_documentation: self.documentation_uri,
            additional_metadata: HashMap::new(),
        }
    }
}

/// WWW-Authenticate header builder for 401 Unauthorized and 403 Forbidden responses
///
/// Implements RFC 9728 Section 5.1 "WWW-Authenticate Response" for indicating
/// the location of Protected Resource Metadata, plus the RFC 6750 §3.1 error
/// codes an HTTP transport needs to distinguish a missing/invalid token (401)
/// from a valid token that lacks a required scope (403):
/// [`Self::invalid_token`] and [`Self::insufficient_scope`].
#[derive(Debug, Clone)]
pub struct WwwAuthenticateBuilder {
    /// Resource metadata URI for .well-known endpoint
    metadata_uri: String,
    /// Scope required for this resource
    scope: Option<String>,
    /// Error code (if applicable)
    error: Option<String>,
    /// Error description
    error_description: Option<String>,
}

impl WwwAuthenticateBuilder {
    /// Create a new WWW-Authenticate builder
    pub fn new(metadata_uri: String) -> Self {
        Self {
            metadata_uri,
            scope: None,
            error: None,
            error_description: None,
        }
    }

    /// Build a 401 challenge for a missing, malformed, expired, or otherwise
    /// invalid bearer token (RFC 6750 §3.1 `invalid_token`).
    pub fn invalid_token(metadata_uri: String, description: Option<String>) -> Self {
        Self::new(metadata_uri).with_error("invalid_token".to_string(), description)
    }

    /// Build a 403 challenge for a structurally valid token that lacks a
    /// required scope (RFC 6750 §3.1 `insufficient_scope`). `scope` is the
    /// scope(s) (space-separated) the caller needs to retry with.
    pub fn insufficient_scope(
        metadata_uri: String,
        scope: String,
        description: Option<String>,
    ) -> Self {
        Self::new(metadata_uri)
            .with_scope(scope)
            .with_error("insufficient_scope".to_string(), description)
    }

    /// Set required scope
    pub fn with_scope(mut self, scope: String) -> Self {
        self.scope = Some(scope);
        self
    }

    /// Set error code and description
    pub fn with_error(mut self, error: String, description: Option<String>) -> Self {
        self.error = Some(error);
        self.error_description = description;
        self
    }

    /// Build the WWW-Authenticate header value
    ///
    /// Produces a header like:
    /// ```text
    /// Bearer resource_metadata="https://api.example.com/.well-known/protected-resource", scope="openid profile"
    /// ```
    pub fn build(self) -> String {
        let mut parts = vec![format!(
            "Bearer resource_metadata=\"{}\"",
            self.metadata_uri
        )];

        if let Some(scope) = self.scope {
            parts.push(format!("scope=\"{}\"", scope));
        }

        if let Some(error) = self.error {
            parts.push(format!("error=\"{}\"", error));
        }

        if let Some(description) = self.error_description {
            parts.push(format!("error_description=\"{}\"", description));
        }

        parts.join(", ")
    }
}

/// Token validation helper for bearer token extraction and validation
#[derive(Debug, Clone)]
pub struct BearerTokenValidator;

impl BearerTokenValidator {
    /// Extract bearer token from Authorization header
    ///
    /// # Arguments
    /// * `authorization_header` - The Authorization header value (e.g., "Bearer token123")
    ///
    /// # Returns
    /// The extracted token, or an error if the header format is invalid
    ///
    /// # Example
    /// ```no_run
    /// # use turbomcp_auth::server::BearerTokenValidator;
    /// let token = BearerTokenValidator::extract_from_header("Bearer mytoken")?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn extract_from_header(authorization_header: &str) -> McpResult<String> {
        let parts: Vec<&str> = authorization_header.split_whitespace().collect();

        if parts.len() != 2 {
            return Err(McpError::invalid_params(
                "Authorization header must have format: Bearer <token>".to_string(),
            ));
        }

        if parts[0].to_lowercase() != "bearer" {
            return Err(McpError::invalid_params(
                "Only Bearer token authentication is supported".to_string(),
            ));
        }

        Ok(parts[1].to_string())
    }

    /// Validate token format (basic checks only)
    ///
    /// This performs basic structural validation. For security-critical operations,
    /// always validate tokens with the authorization server.
    pub fn validate_format(token: &str) -> McpResult<()> {
        if token.is_empty() {
            return Err(McpError::invalid_params("Token is empty".to_string()));
        }

        if token.len() < 10 {
            return Err(McpError::invalid_params("Token is too short".to_string()));
        }

        if token.len() > 10000 {
            return Err(McpError::invalid_params("Token is too long".to_string()));
        }

        Ok(())
    }
}

/// Outcome of a failed [`validate_bearer_token`] call, already distinguishing
/// the two HTTP statuses RFC 6750 §3.1 requires a resource server to choose
/// between: an invalid token gets 401, a valid token missing a required
/// scope gets 403.
#[derive(Debug)]
pub enum TokenValidationError {
    /// Token missing, malformed, expired, or otherwise fails signature,
    /// audience, issuer, or timestamp validation. Respond 401 with
    /// [`WwwAuthenticateBuilder::invalid_token`].
    InvalidToken(McpError),
    /// Token is structurally valid but lacks a scope the request requires.
    /// Respond 403 with [`WwwAuthenticateBuilder::insufficient_scope`].
    InsufficientScope {
        /// Scopes the request required
        required: Vec<String>,
        /// Scopes the token actually carried
        granted: Vec<String>,
    },
}

impl std::fmt::Display for TokenValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidToken(e) => write!(f, "invalid_token: {e}"),
            Self::InsufficientScope { required, granted } => write!(
                f,
                "insufficient_scope: requires [{}], token has [{}]",
                required.join(", "),
                granted.join(", ")
            ),
        }
    }
}

impl std::error::Error for TokenValidationError {}

/// Validate a bearer token and enforce required scopes in one call — the
/// entry point an HTTP transport integration should call for every incoming
/// request that needs auth.
///
/// Wraps [`crate::jwt::JwtValidator::validate_with_refresh`] (signature,
/// audience, issuer, exp/nbf — see [`crate::jwt::JwtValidator`] for how to
/// construct one, including JWKS discovery) and layers RFC 6749 §3.3 scope
/// enforcement on top, reading the `scope` claim (space-separated, per RFC
/// 6749) from the validated token.
///
/// # Errors
///
/// Returns [`TokenValidationError::InvalidToken`] if the token itself is
/// invalid, or [`TokenValidationError::InsufficientScope`] if it's valid but
/// missing a scope in `required_scopes`. The caller can match on the variant
/// to pick 401 vs. 403 and build the matching [`WwwAuthenticateBuilder`]
/// challenge.
///
/// # Example
///
/// ```no_run
/// # async fn example(validator: &turbomcp_auth::jwt::JwtValidator, token: &str) {
/// use turbomcp_auth::server::{TokenValidationError, WwwAuthenticateBuilder, validate_bearer_token};
///
/// let metadata_uri = "https://mcp.example.com/.well-known/oauth-protected-resource";
/// match validate_bearer_token(validator, token, &["mcp:tools:read"]).await {
///     Ok(auth_context) => { /* proceed with auth_context */ }
///     Err(TokenValidationError::InvalidToken(e)) => {
///         // 401 Unauthorized
///         let _header = WwwAuthenticateBuilder::invalid_token(
///             metadata_uri.to_string(),
///             Some(e.to_string()),
///         )
///         .build();
///     }
///     Err(TokenValidationError::InsufficientScope { required, .. }) => {
///         // 403 Forbidden
///         let _header = WwwAuthenticateBuilder::insufficient_scope(
///             metadata_uri.to_string(),
///             required.join(" "),
///             None,
///         )
///         .build();
///     }
/// }
/// # }
/// ```
pub async fn validate_bearer_token(
    validator: &crate::jwt::JwtValidator,
    token: &str,
    required_scopes: &[&str],
) -> Result<crate::context::AuthContext, TokenValidationError> {
    let result = validator
        .validate_with_refresh(token)
        .await
        .map_err(TokenValidationError::InvalidToken)?;

    let claims = result.claims;
    let granted: Vec<String> = claims
        .additional
        .get("scope")
        .and_then(Value::as_str)
        .map(|s| s.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default();

    let subject = claims.sub.unwrap_or_default();
    let mut builder = crate::context::AuthContext::builder()
        .subject(subject.clone())
        .user(crate::types::UserInfo {
            id: subject.clone(),
            username: subject,
            email: None,
            display_name: None,
            avatar_url: None,
            metadata: HashMap::new(),
        })
        .provider("jwt")
        .scopes(granted.clone())
        .authenticated_at(std::time::SystemTime::now());

    if let Some(iss) = claims.iss {
        builder = builder.iss(iss);
    }
    if let Some(aud) = claims.aud.and_then(|values| values.into_iter().next()) {
        builder = builder.aud(aud);
    }
    if let Some(exp) = claims.exp {
        builder = builder.exp(exp);
    }
    if let Some(jti) = claims.jti {
        builder = builder.jti(jti);
    }

    let auth_context = builder
        .build()
        .map_err(|e| TokenValidationError::InvalidToken(McpError::internal(e.to_string())))?;

    if !required_scopes.is_empty() && !auth_context.has_all_scopes(required_scopes) {
        return Err(TokenValidationError::InsufficientScope {
            required: required_scopes.iter().map(|s| s.to_string()).collect(),
            granted,
        });
    }

    Ok(auth_context)
}

/// Build a 401 Unauthorized JSON response body
pub fn unauthorized_response_body(metadata_uri: &str, scope: Option<&str>) -> Value {
    let mut response = json!({
        "error": "unauthorized",
        "error_description": "Valid bearer token required",
        "metadata_uri": metadata_uri,
    });

    if let Some(s) = scope {
        response["required_scope"] = Value::String(s.to_string());
    }

    response
}

/// Validate that a token's audience matches the server's canonical URI
///
/// Per RFC 8707 (Resource Indicators) and MCP spec, access tokens must be bound
/// to their intended audience to prevent confused deputy attacks.
///
/// # Normalization Rules (RFC 8707 Section 2)
///
/// - Scheme and host are case-insensitive (lowercase comparison)
/// - Trailing slash is optional (normalized away)
/// - Port is significant (must match if present)
/// - Path is significant (exact match after normalization)
///
/// # Arguments
///
/// * `token_aud` - Audience claim from the JWT (aud claim)
/// * `server_uri` - Server's canonical resource URI
///
/// # Errors
///
/// Returns error if:
/// - Audience doesn't match server URI
/// - Invalid URI format
///
/// # Examples
///
/// ```rust
/// use turbomcp_auth::server::validate_audience;
///
/// // These all match:
/// assert!(validate_audience("https://api.example.com", "https://api.example.com").is_ok());
/// assert!(validate_audience("https://api.example.com/", "https://api.example.com").is_ok());
/// assert!(validate_audience("https://API.EXAMPLE.COM", "https://api.example.com").is_ok());
///
/// // These don't match:
/// assert!(validate_audience("https://api.example.com:8080", "https://api.example.com").is_err());
/// assert!(validate_audience("https://api.example.com/path", "https://api.example.com").is_err());
/// ```
pub fn validate_audience(token_aud: &str, server_uri: &str) -> turbomcp_protocol::Result<()> {
    use url::Url;

    let token_url = Url::parse(token_aud).map_err(|e| {
        turbomcp_protocol::Error::invalid_params(format!("Invalid token audience URI: {}", e))
    })?;

    let server_url = Url::parse(server_uri).map_err(|e| {
        turbomcp_protocol::Error::invalid_params(format!("Invalid server URI: {}", e))
    })?;

    // Normalize per RFC 8707
    let token_normalized = normalize_resource_uri(&token_url);
    let server_normalized = normalize_resource_uri(&server_url);

    // SECURITY: Use constant-time comparison to prevent timing attacks
    let matches: bool =
        subtle::ConstantTimeEq::ct_eq(token_normalized.as_bytes(), server_normalized.as_bytes())
            .into();

    if !matches {
        return Err(turbomcp_protocol::Error::invalid_params(format!(
            "Token audience '{}' does not match server URI '{}' (normalized: '{}' vs '{}')",
            token_aud, server_uri, token_normalized, server_normalized
        )));
    }

    Ok(())
}

/// Normalize a resource URI per RFC 8707 Section 2
///
/// Normalization rules:
/// - Lowercase scheme and host
/// - Remove default ports (80 for http, 443 for https)
/// - Trim trailing slash from path
fn normalize_resource_uri(url: &url::Url) -> String {
    let mut normalized = String::new();

    // Scheme (lowercase)
    normalized.push_str(&url.scheme().to_lowercase());
    normalized.push_str("://");

    // Host (lowercase)
    if let Some(host) = url.host_str() {
        normalized.push_str(&host.to_lowercase());
    }

    // Port (only if non-default)
    if let Some(port) = url.port() {
        let default_port = match url.scheme() {
            "http" => 80,
            "https" => 443,
            _ => 0,
        };

        if port != default_port {
            normalized.push(':');
            normalized.push_str(&port.to_string());
        }
    }

    // Path (exact, but trim trailing slash)
    let path = url.path();
    if path != "/" {
        normalized.push_str(path.trim_end_matches('/'));
    }

    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_builder() {
        let metadata = ProtectedResourceMetadataBuilder::new(
            "https://api.example.com".to_string(),
            "https://auth.example.com".to_string(),
        )
        .with_scopes(vec!["openid".to_string(), "profile".to_string()])
        .with_documentation("https://api.example.com/docs".to_string())
        .build();

        assert_eq!(metadata["resource"], "https://api.example.com");
        // AU-16: RFC 9728 §2 defines this as an array, plural.
        assert_eq!(
            metadata["authorization_servers"],
            serde_json::json!(["https://auth.example.com"])
        );
    }

    /// AU-16: multiple authorization servers serialize as a JSON array.
    #[test]
    fn test_metadata_builder_multiple_authorization_servers() {
        let metadata = ProtectedResourceMetadataBuilder::new(
            "https://api.example.com".to_string(),
            "https://auth-primary.example.com".to_string(),
        )
        .with_additional_authorization_server("https://auth-secondary.example.com".to_string())
        .build();

        assert_eq!(
            metadata["authorization_servers"],
            serde_json::json!([
                "https://auth-primary.example.com",
                "https://auth-secondary.example.com"
            ])
        );
    }

    #[test]
    fn test_www_authenticate_builder() {
        let header = WwwAuthenticateBuilder::new(
            "https://api.example.com/.well-known/protected-resource".to_string(),
        )
        .with_scope("openid profile".to_string())
        .build();

        assert!(header.contains("Bearer"));
        assert!(header.contains("resource_metadata"));
        assert!(header.contains("scope"));
    }

    /// The 401 case: `error="invalid_token"`, no scope.
    #[test]
    fn test_www_authenticate_invalid_token() {
        let header = WwwAuthenticateBuilder::invalid_token(
            "https://api.example.com/.well-known/oauth-protected-resource".to_string(),
            Some("Token expired".to_string()),
        )
        .build();

        assert!(header.contains("error=\"invalid_token\""));
        assert!(header.contains("error_description=\"Token expired\""));
        assert!(header.contains("resource_metadata"));
    }

    /// The 403 case: `error="insufficient_scope"` plus the scope the client needs.
    #[test]
    fn test_www_authenticate_insufficient_scope() {
        let header = WwwAuthenticateBuilder::insufficient_scope(
            "https://api.example.com/.well-known/oauth-protected-resource".to_string(),
            "mcp:tools:write".to_string(),
            None,
        )
        .build();

        assert!(header.contains("error=\"insufficient_scope\""));
        assert!(header.contains("scope=\"mcp:tools:write\""));
    }

    #[test]
    fn test_bearer_token_extraction() {
        let token = BearerTokenValidator::extract_from_header("Bearer mytoken123")
            .expect("Failed to extract token");
        assert_eq!(token, "mytoken123");
    }

    #[test]
    fn test_bearer_token_extraction_case_insensitive() {
        let token = BearerTokenValidator::extract_from_header("bearer mytoken123")
            .expect("Failed to extract token");
        assert_eq!(token, "mytoken123");
    }

    #[test]
    fn test_bearer_token_extraction_invalid_format() {
        let result = BearerTokenValidator::extract_from_header("mytoken123");
        assert!(result.is_err());
    }

    #[test]
    fn test_unauthorized_response() {
        let response = unauthorized_response_body(
            "https://api.example.com/.well-known/protected-resource",
            Some("openid"),
        );

        assert_eq!(response["error"], "unauthorized");
        assert!(response.get("metadata_uri").is_some());
    }

    #[test]
    fn test_audience_validation_exact_match() {
        assert!(validate_audience("https://api.example.com", "https://api.example.com").is_ok());
    }

    #[test]
    fn test_audience_validation_trailing_slash() {
        assert!(validate_audience("https://api.example.com/", "https://api.example.com").is_ok());
        assert!(validate_audience("https://api.example.com", "https://api.example.com/").is_ok());
    }

    #[test]
    fn test_audience_validation_case_insensitive() {
        assert!(validate_audience("https://API.EXAMPLE.COM", "https://api.example.com").is_ok());
        assert!(validate_audience("HTTPS://api.example.com", "https://api.example.com").is_ok());
    }

    #[test]
    fn test_audience_validation_port_mismatch() {
        assert!(
            validate_audience("https://api.example.com:8080", "https://api.example.com").is_err()
        );
    }

    #[test]
    fn test_audience_validation_path_significant() {
        assert!(
            validate_audience("https://api.example.com/mcp", "https://api.example.com").is_err()
        );
        assert!(
            validate_audience("https://api.example.com", "https://api.example.com/mcp").is_err()
        );
    }

    #[test]
    fn test_audience_validation_default_ports() {
        // Default ports should be normalized away
        assert!(
            validate_audience("https://api.example.com:443", "https://api.example.com").is_ok()
        );
        assert!(validate_audience("http://api.example.com:80", "http://api.example.com").is_ok());
    }

    #[test]
    fn test_normalize_resource_uri() {
        use url::Url;

        let url = Url::parse("https://API.EXAMPLE.COM:443/path/").unwrap();
        assert_eq!(normalize_resource_uri(&url), "https://api.example.com/path");

        let url = Url::parse("http://example.com:80").unwrap();
        assert_eq!(normalize_resource_uri(&url), "http://example.com");

        let url = Url::parse("https://example.com:8443/").unwrap();
        assert_eq!(normalize_resource_uri(&url), "https://example.com:8443");
    }
}
