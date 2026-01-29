//! Core types for OAuth 2.1 provider.
//!
//! This module defines the configuration and client types used by the OAuth provider.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// OAuth 2.1 grant types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantType {
    /// Authorization code grant (RFC 6749 Section 4.1)
    AuthorizationCode,
    /// Refresh token grant (RFC 6749 Section 6)
    RefreshToken,
    /// Client credentials grant (RFC 6749 Section 4.4)
    ClientCredentials,
}

impl GrantType {
    /// Get the grant type as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AuthorizationCode => "authorization_code",
            Self::RefreshToken => "refresh_token",
            Self::ClientCredentials => "client_credentials",
        }
    }
}

impl std::fmt::Display for GrantType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// OAuth 2.1 response types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseType {
    /// Authorization code response
    Code,
}

impl ResponseType {
    /// Get the response type as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Code => "code",
        }
    }
}

/// PKCE code challenge methods (RFC 7636).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CodeChallengeMethod {
    /// Plain (not recommended, only for legacy support)
    #[serde(rename = "plain")]
    Plain,
    /// SHA-256 (recommended)
    #[serde(rename = "S256")]
    S256,
}

impl CodeChallengeMethod {
    /// Get the method as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::S256 => "S256",
        }
    }
}

impl Default for CodeChallengeMethod {
    fn default() -> Self {
        Self::S256
    }
}

/// Client authentication method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientAuthMethod {
    /// No authentication (public client, requires PKCE)
    None,
    /// Client secret in POST body
    ClientSecretPost,
    /// Client secret in Authorization header (Basic auth)
    ClientSecretBasic,
}

impl Default for ClientAuthMethod {
    fn default() -> Self {
        Self::None
    }
}

/// Configuration for an OAuth client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    /// Client identifier
    pub client_id: String,

    /// Client secret (for confidential clients)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,

    /// Allowed redirect URIs
    pub redirect_uris: Vec<String>,

    /// Allowed grant types
    #[serde(default)]
    pub grant_types: Vec<GrantType>,

    /// Client authentication method
    #[serde(default)]
    pub auth_method: ClientAuthMethod,

    /// Whether PKCE is required (mandatory for public clients)
    #[serde(default = "default_pkce_required")]
    pub pkce_required: bool,

    /// Client name (for display)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Client homepage URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage_url: Option<String>,

    /// Allowed scopes
    #[serde(default)]
    pub allowed_scopes: Vec<String>,

    /// Default scopes (used when none requested)
    #[serde(default)]
    pub default_scopes: Vec<String>,
}

fn default_pkce_required() -> bool {
    true
}

impl ClientConfig {
    /// Create a new public client configuration.
    ///
    /// Public clients (no secret) require PKCE for security.
    pub fn public(client_id: impl Into<String>, redirect_uris: Vec<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: None,
            redirect_uris,
            grant_types: vec![GrantType::AuthorizationCode, GrantType::RefreshToken],
            auth_method: ClientAuthMethod::None,
            pkce_required: true, // Always required for public clients
            name: None,
            homepage_url: None,
            allowed_scopes: Vec::new(),
            default_scopes: Vec::new(),
        }
    }

    /// Create a new confidential client configuration.
    ///
    /// Confidential clients have a secret and PKCE is optional but recommended.
    pub fn confidential(
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        redirect_uris: Vec<String>,
    ) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: Some(client_secret.into()),
            redirect_uris,
            grant_types: vec![GrantType::AuthorizationCode, GrantType::RefreshToken],
            auth_method: ClientAuthMethod::ClientSecretBasic,
            pkce_required: false, // Optional for confidential clients
            name: None,
            homepage_url: None,
            allowed_scopes: Vec::new(),
            default_scopes: Vec::new(),
        }
    }

    /// Add a grant type.
    pub fn with_grant_type(mut self, grant_type: GrantType) -> Self {
        if !self.grant_types.contains(&grant_type) {
            self.grant_types.push(grant_type);
        }
        self
    }

    /// Set allowed scopes.
    pub fn with_allowed_scopes(mut self, scopes: Vec<String>) -> Self {
        self.allowed_scopes = scopes;
        self
    }

    /// Set default scopes.
    pub fn with_default_scopes(mut self, scopes: Vec<String>) -> Self {
        self.default_scopes = scopes;
        self
    }

    /// Set client name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Check if a redirect URI is allowed.
    pub fn is_redirect_uri_allowed(&self, uri: &str) -> bool {
        self.redirect_uris.iter().any(|allowed| allowed == uri)
    }

    /// Check if a grant type is allowed.
    pub fn is_grant_type_allowed(&self, grant_type: GrantType) -> bool {
        self.grant_types.contains(&grant_type)
    }

    /// Check if a scope is allowed.
    pub fn is_scope_allowed(&self, scope: &str) -> bool {
        self.allowed_scopes.is_empty() || self.allowed_scopes.iter().any(|s| s == scope)
    }

    /// Filter requested scopes to only allowed ones.
    pub fn filter_scopes(&self, requested: &[String]) -> Vec<String> {
        if self.allowed_scopes.is_empty() {
            // No restrictions
            requested.to_vec()
        } else {
            requested
                .iter()
                .filter(|s| self.allowed_scopes.contains(s))
                .cloned()
                .collect()
        }
    }

    /// Get effective scopes (requested or default).
    pub fn effective_scopes(&self, requested: &[String]) -> Vec<String> {
        if requested.is_empty() {
            self.default_scopes.clone()
        } else {
            self.filter_scopes(requested)
        }
    }
}

/// OAuth provider configuration.
#[derive(Debug, Clone)]
pub struct OAuthProviderConfig {
    /// Issuer identifier (e.g., "https://my-server.workers.dev")
    pub issuer: String,

    /// Authorization endpoint path (default: "/oauth/authorize")
    pub authorization_endpoint: String,

    /// Token endpoint path (default: "/oauth/token")
    pub token_endpoint: String,

    /// Revocation endpoint path (default: "/oauth/revoke")
    pub revocation_endpoint: String,

    /// Introspection endpoint path (default: "/oauth/introspect")
    pub introspection_endpoint: String,

    /// JWKS endpoint path (default: "/.well-known/jwks.json")
    pub jwks_endpoint: String,

    /// Authorization code lifetime in seconds (default: 600 = 10 minutes)
    pub authorization_code_lifetime: u64,

    /// Access token lifetime in seconds (default: 3600 = 1 hour)
    pub access_token_lifetime: u64,

    /// Refresh token lifetime in seconds (default: 2592000 = 30 days)
    pub refresh_token_lifetime: u64,

    /// Whether to issue refresh tokens (default: true)
    pub issue_refresh_tokens: bool,

    /// Token signing algorithm (default: RS256)
    pub signing_algorithm: String,

    /// Registered clients
    pub clients: HashMap<String, ClientConfig>,

    /// Supported scopes
    pub supported_scopes: Vec<String>,
}

impl Default for OAuthProviderConfig {
    fn default() -> Self {
        Self {
            issuer: String::new(),
            authorization_endpoint: "/oauth/authorize".to_string(),
            token_endpoint: "/oauth/token".to_string(),
            revocation_endpoint: "/oauth/revoke".to_string(),
            introspection_endpoint: "/oauth/introspect".to_string(),
            jwks_endpoint: "/.well-known/jwks.json".to_string(),
            authorization_code_lifetime: 600,
            access_token_lifetime: 3600,
            refresh_token_lifetime: 2592000,
            issue_refresh_tokens: true,
            signing_algorithm: "RS256".to_string(),
            clients: HashMap::new(),
            supported_scopes: Vec::new(),
        }
    }
}

impl OAuthProviderConfig {
    /// Create a new OAuth provider configuration.
    pub fn new(issuer: impl Into<String>) -> Self {
        Self {
            issuer: issuer.into(),
            ..Default::default()
        }
    }

    /// Register a client.
    pub fn with_client(mut self, config: ClientConfig) -> Self {
        self.clients.insert(config.client_id.clone(), config);
        self
    }

    /// Set supported scopes.
    pub fn with_scopes(mut self, scopes: Vec<String>) -> Self {
        self.supported_scopes = scopes;
        self
    }

    /// Set access token lifetime.
    pub fn with_access_token_lifetime(mut self, seconds: u64) -> Self {
        self.access_token_lifetime = seconds;
        self
    }

    /// Set refresh token lifetime.
    pub fn with_refresh_token_lifetime(mut self, seconds: u64) -> Self {
        self.refresh_token_lifetime = seconds;
        self
    }

    /// Get client by ID.
    pub fn get_client(&self, client_id: &str) -> Option<&ClientConfig> {
        self.clients.get(client_id)
    }

    /// Get the full authorization endpoint URL.
    pub fn authorization_endpoint_url(&self) -> String {
        format!("{}{}", self.issuer, self.authorization_endpoint)
    }

    /// Get the full token endpoint URL.
    pub fn token_endpoint_url(&self) -> String {
        format!("{}{}", self.issuer, self.token_endpoint)
    }

    /// Get the full JWKS endpoint URL.
    pub fn jwks_endpoint_url(&self) -> String {
        format!("{}{}", self.issuer, self.jwks_endpoint)
    }
}

/// OAuth error response (RFC 6749 Section 5.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthError {
    /// Error code
    pub error: String,

    /// Human-readable error description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_description: Option<String>,

    /// Error URI for more information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_uri: Option<String>,

    /// State parameter (for authorization errors)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

impl OAuthError {
    /// Create an invalid_request error.
    pub fn invalid_request(description: impl Into<String>) -> Self {
        Self {
            error: "invalid_request".to_string(),
            error_description: Some(description.into()),
            error_uri: None,
            state: None,
        }
    }

    /// Create an unauthorized_client error.
    pub fn unauthorized_client(description: impl Into<String>) -> Self {
        Self {
            error: "unauthorized_client".to_string(),
            error_description: Some(description.into()),
            error_uri: None,
            state: None,
        }
    }

    /// Create an access_denied error.
    pub fn access_denied(description: impl Into<String>) -> Self {
        Self {
            error: "access_denied".to_string(),
            error_description: Some(description.into()),
            error_uri: None,
            state: None,
        }
    }

    /// Create an unsupported_response_type error.
    pub fn unsupported_response_type(description: impl Into<String>) -> Self {
        Self {
            error: "unsupported_response_type".to_string(),
            error_description: Some(description.into()),
            error_uri: None,
            state: None,
        }
    }

    /// Create an invalid_scope error.
    pub fn invalid_scope(description: impl Into<String>) -> Self {
        Self {
            error: "invalid_scope".to_string(),
            error_description: Some(description.into()),
            error_uri: None,
            state: None,
        }
    }

    /// Create a server_error error.
    pub fn server_error(description: impl Into<String>) -> Self {
        Self {
            error: "server_error".to_string(),
            error_description: Some(description.into()),
            error_uri: None,
            state: None,
        }
    }

    /// Create an invalid_grant error.
    pub fn invalid_grant(description: impl Into<String>) -> Self {
        Self {
            error: "invalid_grant".to_string(),
            error_description: Some(description.into()),
            error_uri: None,
            state: None,
        }
    }

    /// Create an invalid_client error.
    pub fn invalid_client(description: impl Into<String>) -> Self {
        Self {
            error: "invalid_client".to_string(),
            error_description: Some(description.into()),
            error_uri: None,
            state: None,
        }
    }

    /// Create an unsupported_grant_type error.
    pub fn unsupported_grant_type(description: impl Into<String>) -> Self {
        Self {
            error: "unsupported_grant_type".to_string(),
            error_description: Some(description.into()),
            error_uri: None,
            state: None,
        }
    }

    /// Set the state parameter.
    pub fn with_state(mut self, state: impl Into<String>) -> Self {
        self.state = Some(state.into());
        self
    }
}

/// Token response (RFC 6749 Section 5.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    /// Access token
    pub access_token: String,

    /// Token type (always "Bearer")
    pub token_type: String,

    /// Expiration time in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,

    /// Refresh token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,

    /// Granted scopes (if different from request)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

impl TokenResponse {
    /// Create a new token response.
    pub fn new(access_token: impl Into<String>) -> Self {
        Self {
            access_token: access_token.into(),
            token_type: "Bearer".to_string(),
            expires_in: None,
            refresh_token: None,
            scope: None,
        }
    }

    /// Set expiration time.
    pub fn with_expires_in(mut self, seconds: u64) -> Self {
        self.expires_in = Some(seconds);
        self
    }

    /// Set refresh token.
    pub fn with_refresh_token(mut self, token: impl Into<String>) -> Self {
        self.refresh_token = Some(token.into());
        self
    }

    /// Set granted scopes.
    pub fn with_scope(mut self, scopes: &[String]) -> Self {
        if !scopes.is_empty() {
            self.scope = Some(scopes.join(" "));
        }
        self
    }
}

/// Token introspection response (RFC 7662).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntrospectionResponse {
    /// Whether the token is active
    pub active: bool,

    /// Scopes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,

    /// Client ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Subject
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,

    /// Expiration time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<u64>,

    /// Issued at
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<u64>,

    /// Token type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
}

impl IntrospectionResponse {
    /// Create an inactive response.
    pub fn inactive() -> Self {
        Self {
            active: false,
            scope: None,
            client_id: None,
            sub: None,
            exp: None,
            iat: None,
            token_type: None,
        }
    }

    /// Create an active response.
    pub fn active(
        subject: impl Into<String>,
        client_id: impl Into<String>,
        scopes: &[String],
        expires_at: u64,
        issued_at: u64,
    ) -> Self {
        Self {
            active: true,
            scope: if scopes.is_empty() {
                None
            } else {
                Some(scopes.join(" "))
            },
            client_id: Some(client_id.into()),
            sub: Some(subject.into()),
            exp: Some(expires_at),
            iat: Some(issued_at),
            token_type: Some("Bearer".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_public_client_requires_pkce() {
        let client = ClientConfig::public(
            "my-client",
            vec!["https://app.example.com/callback".to_string()],
        );
        assert!(client.pkce_required);
        assert!(client.client_secret.is_none());
    }

    #[test]
    fn test_confidential_client_has_secret() {
        let client = ClientConfig::confidential(
            "my-client",
            "secret123",
            vec!["https://app.example.com/callback".to_string()],
        );
        assert!(!client.pkce_required);
        assert!(client.client_secret.is_some());
    }

    #[test]
    fn test_redirect_uri_validation() {
        let client = ClientConfig::public(
            "my-client",
            vec![
                "https://app.example.com/callback".to_string(),
                "https://app.example.com/callback2".to_string(),
            ],
        );

        assert!(client.is_redirect_uri_allowed("https://app.example.com/callback"));
        assert!(client.is_redirect_uri_allowed("https://app.example.com/callback2"));
        assert!(!client.is_redirect_uri_allowed("https://evil.com/callback"));
    }

    #[test]
    fn test_scope_filtering() {
        let client = ClientConfig::public("my-client", vec![])
            .with_allowed_scopes(vec!["read".to_string(), "write".to_string()]);

        let filtered = client.filter_scopes(&[
            "read".to_string(),
            "admin".to_string(), // Not allowed
            "write".to_string(),
        ]);

        assert_eq!(filtered, vec!["read", "write"]);
    }

    #[test]
    fn test_oauth_error_types() {
        let err = OAuthError::invalid_request("Missing client_id");
        assert_eq!(err.error, "invalid_request");

        let err = OAuthError::invalid_grant("Code expired").with_state("state123");
        assert_eq!(err.error, "invalid_grant");
        assert_eq!(err.state, Some("state123".to_string()));
    }

    #[test]
    fn test_token_response() {
        let response = TokenResponse::new("access_token_123")
            .with_expires_in(3600)
            .with_refresh_token("refresh_token_456")
            .with_scope(&["read".to_string(), "write".to_string()]);

        assert_eq!(response.access_token, "access_token_123");
        assert_eq!(response.token_type, "Bearer");
        assert_eq!(response.expires_in, Some(3600));
        assert_eq!(
            response.refresh_token,
            Some("refresh_token_456".to_string())
        );
        assert_eq!(response.scope, Some("read write".to_string()));
    }
}
