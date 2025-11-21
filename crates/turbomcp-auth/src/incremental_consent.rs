//! # Incremental Scope Consent (SEP-835)
//!
//! Support for OAuth 2.0 incremental authorization using WWW-Authenticate headers
//! as specified in MCP 2025-11-25 (SEP-835) and RFC 6750.
//!
//! ## Overview
//!
//! Incremental authorization allows MCP servers to request additional scopes
//! from clients dynamically as needed, rather than requiring all permissions
//! upfront. This improves user experience and follows the principle of least
//! privilege.
//!
//! ## How It Works
//!
//! 1. Client makes a request with limited scopes
//! 2. Server determines additional scopes are needed
//! 3. Server responds with 401 + WWW-Authenticate header specifying required scopes
//! 4. Client initiates new OAuth flow to obtain token with additional scopes
//! 5. Client retries original request with enhanced token
//!
//! ## WWW-Authenticate Header Format
//!
//! Per RFC 6750 Section 3, the WWW-Authenticate header for Bearer tokens uses:
//!
//! ```text
//! WWW-Authenticate: Bearer realm="Example", scope="write:data read:profile", error="insufficient_scope"
//! ```
//!
//! ## Usage Example
//!
//! ```rust
//! use turbomcp_auth::incremental_consent::{IncrementalConsentChallenge, OAuth2Error};
//!
//! // Server needs additional scopes
//! let challenge = IncrementalConsentChallenge::builder()
//!     .realm("MCP Server API")
//!     .scopes(vec!["write:tools".to_string(), "read:resources".to_string()])
//!     .error(OAuth2Error::InsufficientScope)
//!     .error_description("Additional permissions required to access this tool")
//!     .error_uri("https://docs.example.com/oauth/scopes")
//!     .build();
//!
//! // Generate WWW-Authenticate header value
//! let header_value = challenge.to_header_value();
//! // "Bearer realm=\"MCP Server API\", scope=\"write:tools read:resources\", error=\"insufficient_scope\", error_description=\"...\""
//! ```
//!
//! ## Standards Compliance
//!
//! - **RFC 6750**: OAuth 2.0 Bearer Token Usage (WWW-Authenticate)
//! - **draft-ietf-oauth-incremental-authz**: OAuth 2.0 Incremental Authorization
//! - **MCP SEP-835**: Incremental Scope Consent via WWW-Authenticate

use std::fmt;

/// Standard OAuth 2.0 error codes per RFC 6750 Section 3.1
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuth2Error {
    /// The request is missing a required parameter, includes an unsupported
    /// parameter or parameter value, repeats the same parameter, uses more
    /// than one method for including an access token, or is otherwise malformed.
    InvalidRequest,

    /// The access token provided is expired, revoked, malformed, or invalid
    /// for other reasons.
    InvalidToken,

    /// The request requires higher privileges than provided by the access token.
    /// This is the primary error code for incremental authorization.
    InsufficientScope,
}

impl OAuth2Error {
    /// Get the error code string per RFC 6750
    pub fn as_str(&self) -> &'static str {
        match self {
            OAuth2Error::InvalidRequest => "invalid_request",
            OAuth2Error::InvalidToken => "invalid_token",
            OAuth2Error::InsufficientScope => "insufficient_scope",
        }
    }
}

impl fmt::Display for OAuth2Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Incremental consent challenge for WWW-Authenticate header
///
/// Represents an OAuth 2.0 Bearer token challenge with support for
/// incremental authorization (requesting additional scopes).
#[derive(Debug, Clone)]
pub struct IncrementalConsentChallenge {
    /// The authentication realm (optional but recommended)
    realm: Option<String>,

    /// Required scopes (space-separated in header)
    scopes: Vec<String>,

    /// Error code (per RFC 6750)
    error: Option<OAuth2Error>,

    /// Human-readable error description
    error_description: Option<String>,

    /// URI for error documentation
    error_uri: Option<String>,

    /// Additional auth-params (extensibility)
    additional_params: Vec<(String, String)>,
}

impl IncrementalConsentChallenge {
    /// Create a new challenge builder
    pub fn builder() -> IncrementalConsentChallengeBuilder {
        IncrementalConsentChallengeBuilder::default()
    }

    /// Convert to WWW-Authenticate header value
    ///
    /// Produces a header value like:
    /// ```text
    /// Bearer realm="Example", scope="read write", error="insufficient_scope"
    /// ```
    pub fn to_header_value(&self) -> String {
        let mut parts = vec!["Bearer".to_string()];

        // Add realm
        if let Some(ref realm) = self.realm {
            parts.push(format!("realm=\"{}\"", escape_param_value(realm)));
        }

        // Add scope (space-separated per RFC 6750)
        if !self.scopes.is_empty() {
            let scope_value = self.scopes.join(" ");
            parts.push(format!("scope=\"{}\"", escape_param_value(&scope_value)));
        }

        // Add error
        if let Some(error) = self.error {
            parts.push(format!("error=\"{}\"", error.as_str()));
        }

        // Add error_description
        if let Some(ref desc) = self.error_description {
            parts.push(format!(
                "error_description=\"{}\"",
                escape_param_value(desc)
            ));
        }

        // Add error_uri
        if let Some(ref uri) = self.error_uri {
            parts.push(format!("error_uri=\"{}\"", escape_param_value(uri)));
        }

        // Add additional params
        for (key, value) in &self.additional_params {
            parts.push(format!("{}=\"{}\"", key, escape_param_value(value)));
        }

        // Join with comma-space (RFC 6750 auth-param format)
        if parts.len() == 1 {
            // Just "Bearer" with no params
            parts[0].clone()
        } else {
            format!("{} {}", parts[0], parts[1..].join(", "))
        }
    }

    /// Get the realm
    pub fn realm(&self) -> Option<&str> {
        self.realm.as_deref()
    }

    /// Get the required scopes
    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }

    /// Get the error code
    pub fn error(&self) -> Option<OAuth2Error> {
        self.error
    }

    /// Get the error description
    pub fn error_description(&self) -> Option<&str> {
        self.error_description.as_deref()
    }

    /// Get the error URI
    pub fn error_uri(&self) -> Option<&str> {
        self.error_uri.as_deref()
    }
}

/// Builder for incremental consent challenges
#[derive(Debug, Default)]
pub struct IncrementalConsentChallengeBuilder {
    realm: Option<String>,
    scopes: Vec<String>,
    error: Option<OAuth2Error>,
    error_description: Option<String>,
    error_uri: Option<String>,
    additional_params: Vec<(String, String)>,
}

impl IncrementalConsentChallengeBuilder {
    /// Set the authentication realm
    ///
    /// The realm indicates the scope of protection per HTTP authentication.
    /// Recommended for better UX.
    pub fn realm(mut self, realm: impl Into<String>) -> Self {
        self.realm = Some(realm.into());
        self
    }

    /// Set a single required scope
    pub fn scope(mut self, scope: impl Into<String>) -> Self {
        self.scopes.push(scope.into());
        self
    }

    /// Set multiple required scopes
    pub fn scopes(mut self, scopes: Vec<String>) -> Self {
        self.scopes = scopes;
        self
    }

    /// Add an additional scope to the existing list
    pub fn add_scope(mut self, scope: impl Into<String>) -> Self {
        self.scopes.push(scope.into());
        self
    }

    /// Set the OAuth 2.0 error code
    pub fn error(mut self, error: OAuth2Error) -> Self {
        self.error = Some(error);
        self
    }

    /// Set the error description (human-readable)
    pub fn error_description(mut self, description: impl Into<String>) -> Self {
        self.error_description = Some(description.into());
        self
    }

    /// Set the error documentation URI
    pub fn error_uri(mut self, uri: impl Into<String>) -> Self {
        self.error_uri = Some(uri.into());
        self
    }

    /// Add a custom auth-param
    ///
    /// Allows adding non-standard parameters for extensibility
    pub fn additional_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.additional_params.push((key.into(), value.into()));
        self
    }

    /// Build the challenge
    pub fn build(self) -> IncrementalConsentChallenge {
        IncrementalConsentChallenge {
            realm: self.realm,
            scopes: self.scopes,
            error: self.error,
            error_description: self.error_description,
            error_uri: self.error_uri,
            additional_params: self.additional_params,
        }
    }
}

/// Escape special characters in auth-param values
///
/// Per RFC 2617, quoted-string values need backslash escaping for:
/// - Quotes (")
/// - Backslashes (\)
fn escape_param_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Helper function to create an insufficient_scope challenge
///
/// This is the most common use case for incremental consent.
///
/// # Example
///
/// ```rust
/// use turbomcp_auth::incremental_consent::insufficient_scope_challenge;
///
/// let challenge = insufficient_scope_challenge(
///     "MCP Tools API",
///     vec!["write:tools".to_string()],
///     Some("Tool execution requires write:tools scope"),
/// );
///
/// let header = challenge.to_header_value();
/// ```
pub fn insufficient_scope_challenge(
    realm: impl Into<String>,
    required_scopes: Vec<String>,
    description: Option<&str>,
) -> IncrementalConsentChallenge {
    let mut builder = IncrementalConsentChallenge::builder()
        .realm(realm)
        .scopes(required_scopes)
        .error(OAuth2Error::InsufficientScope);

    if let Some(desc) = description {
        builder = builder.error_description(desc);
    }

    builder.build()
}

/// Helper function to create an invalid_token challenge
///
/// Used when the provided token is expired, revoked, or malformed.
///
/// # Example
///
/// ```rust
/// use turbomcp_auth::incremental_consent::invalid_token_challenge;
///
/// let challenge = invalid_token_challenge(
///     "MCP API",
///     Some("Token has expired"),
/// );
/// ```
pub fn invalid_token_challenge(
    realm: impl Into<String>,
    description: Option<&str>,
) -> IncrementalConsentChallenge {
    let mut builder = IncrementalConsentChallenge::builder()
        .realm(realm)
        .error(OAuth2Error::InvalidToken);

    if let Some(desc) = description {
        builder = builder.error_description(desc);
    }

    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oauth2_error_codes() {
        assert_eq!(OAuth2Error::InvalidRequest.as_str(), "invalid_request");
        assert_eq!(OAuth2Error::InvalidToken.as_str(), "invalid_token");
        assert_eq!(
            OAuth2Error::InsufficientScope.as_str(),
            "insufficient_scope"
        );
    }

    #[test]
    fn test_basic_challenge() {
        let challenge = IncrementalConsentChallenge::builder()
            .realm("Example Realm")
            .build();

        let header = challenge.to_header_value();
        assert_eq!(header, "Bearer realm=\"Example Realm\"");
    }

    #[test]
    fn test_challenge_with_scopes() {
        let challenge = IncrementalConsentChallenge::builder()
            .realm("API")
            .scopes(vec!["read".to_string(), "write".to_string()])
            .build();

        let header = challenge.to_header_value();
        assert!(header.contains("realm=\"API\""));
        assert!(header.contains("scope=\"read write\""));
    }

    #[test]
    fn test_insufficient_scope_challenge() {
        let challenge = IncrementalConsentChallenge::builder()
            .realm("MCP Server")
            .scopes(vec!["write:tools".to_string()])
            .error(OAuth2Error::InsufficientScope)
            .error_description("Additional permissions required")
            .build();

        let header = challenge.to_header_value();
        assert!(header.contains("realm=\"MCP Server\""));
        assert!(header.contains("scope=\"write:tools\""));
        assert!(header.contains("error=\"insufficient_scope\""));
        assert!(header.contains("error_description=\"Additional permissions required\""));
    }

    #[test]
    fn test_challenge_with_error_uri() {
        let challenge = IncrementalConsentChallenge::builder()
            .realm("API")
            .error(OAuth2Error::InvalidToken)
            .error_uri("https://docs.example.com/errors/invalid_token")
            .build();

        let header = challenge.to_header_value();
        assert!(header.contains("error=\"invalid_token\""));
        assert!(header.contains("error_uri=\"https://docs.example.com/errors/invalid_token\""));
    }

    #[test]
    fn test_escape_param_value() {
        let value = "Hello \"World\" with \\backslash";
        let escaped = escape_param_value(value);
        assert_eq!(escaped, "Hello \\\"World\\\" with \\\\backslash");
    }

    #[test]
    fn test_challenge_with_special_characters() {
        let challenge = IncrementalConsentChallenge::builder()
            .realm("Realm with \"quotes\"")
            .error_description("Error with \\backslash")
            .build();

        let header = challenge.to_header_value();
        assert!(header.contains("realm=\"Realm with \\\"quotes\\\"\""));
        assert!(header.contains("error_description=\"Error with \\\\backslash\""));
    }

    #[test]
    fn test_insufficient_scope_helper() {
        let challenge = insufficient_scope_challenge(
            "MCP Tools",
            vec!["write:tools".to_string(), "read:resources".to_string()],
            Some("Tool execution requires additional scopes"),
        );

        assert_eq!(challenge.realm(), Some("MCP Tools"));
        assert_eq!(challenge.scopes(), &["write:tools", "read:resources"]);
        assert_eq!(challenge.error(), Some(OAuth2Error::InsufficientScope));
        assert_eq!(
            challenge.error_description(),
            Some("Tool execution requires additional scopes")
        );
    }

    #[test]
    fn test_invalid_token_helper() {
        let challenge = invalid_token_challenge("MCP API", Some("Token has expired"));

        assert_eq!(challenge.realm(), Some("MCP API"));
        assert_eq!(challenge.error(), Some(OAuth2Error::InvalidToken));
        assert_eq!(challenge.error_description(), Some("Token has expired"));
    }

    #[test]
    fn test_additional_params() {
        let challenge = IncrementalConsentChallenge::builder()
            .realm("API")
            .additional_param("custom_param", "custom_value")
            .build();

        let header = challenge.to_header_value();
        assert!(header.contains("custom_param=\"custom_value\""));
    }

    #[test]
    fn test_bearer_only() {
        let challenge = IncrementalConsentChallenge::builder().build();

        let header = challenge.to_header_value();
        assert_eq!(header, "Bearer");
    }

    #[test]
    fn test_multiple_scopes_ordering() {
        let challenge = IncrementalConsentChallenge::builder()
            .scope("read")
            .scope("write")
            .scope("delete")
            .build();

        let header = challenge.to_header_value();
        assert!(header.contains("scope=\"read write delete\""));
    }
}
