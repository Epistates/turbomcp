//! OAuth 2.0 Dynamic Client Registration (RFC 7591)
//!
//! Enables OAuth clients to register themselves with authorization servers
//! without manual pre-registration.
//!
//! # MCP Specification
//!
//! Per MCP spec (2025-06-18):
//! > MCP auth implementations SHOULD support the OAuth 2.0 Dynamic Client
//! > Registration Protocol (RFC7591).
//!
//! # Why Dynamic Client Registration?
//!
//! - **Seamless Integration**: No manual client registration needed
//! - **Developer Experience**: Auto-configuration for CLI tools, SDKs
//! - **Scalability**: Programmatic client creation
//! - **Security**: Cryptographically secure client secrets
//!
//! # Example
//!
//! ```rust,no_run
//! use turbomcp_auth::oauth2::dcr::{DcrClient, DcrBuilder};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create a DCR client
//! let dcr_client = DcrClient::new(
//!     "https://auth.example.com/register".to_string(),
//!     None, // No initial access token required
//! );
//!
//! // Build registration request for MCP client
//! let request = DcrBuilder::mcp_client(
//!     "My MCP Client",
//!     "http://localhost:3000/callback"
//! )
//! .with_scopes(vec!["mcp:tools".to_string(), "mcp:resources".to_string()])
//! .with_client_uri("https://my-app.example.com".to_string())
//! .build();
//!
//! // Register the client
//! let response = dcr_client.register(request).await?;
//!
//! println!("Client ID: {}", response.client_id);
//! println!("Client Secret: {:?}", response.client_secret);
//! # Ok(())
//! # }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use turbomcp_protocol::{Error as McpError, Result as McpResult};

/// Client registration request per RFC 7591 Section 2
///
/// This structure represents the metadata that a client sends to the
/// authorization server when requesting dynamic registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationRequest {
    /// Redirect URIs (REQUIRED for authorization code flow)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_uris: Option<Vec<String>>,

    /// Token endpoint authentication method
    ///
    /// Common values:
    /// - `client_secret_basic` - HTTP Basic authentication
    /// - `client_secret_post` - Client credentials in POST body
    /// - `none` - Public client (no authentication)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth_method: Option<String>,

    /// Grant types supported by the client
    ///
    /// Common values:
    /// - `authorization_code`
    /// - `refresh_token`
    /// - `client_credentials`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant_types: Option<Vec<String>>,

    /// Response types the client will use
    ///
    /// Common values:
    /// - `code` - Authorization code flow
    /// - `token` - Implicit flow (deprecated)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_types: Option<Vec<String>>,

    /// Human-readable client name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,

    /// Client homepage URI
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_uri: Option<String>,

    /// Logo URI for the client
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_uri: Option<String>,

    /// Space-separated list of OAuth scopes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,

    /// Contact email addresses
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contacts: Option<Vec<String>>,

    /// Terms of service URI
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tos_uri: Option<String>,

    /// Privacy policy URI
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_uri: Option<String>,

    /// Software identifier (for version tracking)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub software_id: Option<String>,

    /// Software version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub software_version: Option<String>,

    /// JWKS URI for public key retrieval
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwks_uri: Option<String>,

    /// Application type (web, native)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_type: Option<String>,
}

/// Client registration response per RFC 7591 Section 3.2
///
/// Contains the registered client credentials and metadata returned
/// by the authorization server.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RegistrationResponse {
    /// Client identifier (REQUIRED)
    pub client_id: String,

    /// Client secret (if confidential client)
    ///
    /// This will be None for public clients (e.g., native apps, SPAs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,

    /// Client secret expiration time (seconds since epoch, 0 = never)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret_expires_at: Option<u64>,

    /// Registration access token (for updating/deleting registration)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registration_access_token: Option<String>,

    /// Registration client URI (for PUT/DELETE operations)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registration_client_uri: Option<String>,

    /// Client ID issued at timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id_issued_at: Option<u64>,

    /// All registered metadata (echo of request + server additions)
    #[serde(flatten)]
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Dynamic Client Registration client
///
/// # Example
///
/// ```rust,no_run
/// use turbomcp_auth::oauth2::dcr::{DcrClient, DcrBuilder};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let client = DcrClient::new(
///     "https://auth.example.com/register".to_string(),
///     None,
/// );
///
/// let request = DcrBuilder::mcp_client("My App", "http://localhost:3000/callback")
///     .with_scopes(vec!["mcp:tools".to_string()])
///     .build();
///
/// let response = client.register(request).await?;
/// println!("Registered! Client ID: {}", response.client_id);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct DcrClient {
    /// Registration endpoint URL
    endpoint: String,

    /// Initial access token (if required by server)
    ///
    /// Some authorization servers require an initial access token
    /// to prevent unauthorized client registration.
    initial_access_token: Option<String>,

    /// HTTP client
    http_client: reqwest::Client,
}

impl DcrClient {
    /// Create a new DCR client
    ///
    /// # Arguments
    ///
    /// * `endpoint` - Registration endpoint URL (from AS metadata `registration_endpoint`)
    /// * `initial_access_token` - Optional token for authenticated registration
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_auth::oauth2::dcr::DcrClient;
    ///
    /// // Open registration (no token required)
    /// let client = DcrClient::new(
    ///     "https://auth.example.com/register".to_string(),
    ///     None,
    /// );
    ///
    /// // Authenticated registration
    /// let auth_client = DcrClient::new(
    ///     "https://auth.example.com/register".to_string(),
    ///     Some("initial_access_token_here".to_string()),
    /// );
    /// ```
    pub fn new(endpoint: String, initial_access_token: Option<String>) -> Self {
        Self {
            endpoint,
            initial_access_token,
            http_client: reqwest::Client::new(),
        }
    }

    /// Register a new OAuth client
    ///
    /// # Arguments
    ///
    /// * `request` - Client registration metadata
    ///
    /// # Returns
    ///
    /// Registration response with client_id, client_secret, and metadata
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - HTTP request fails
    /// - Server rejects registration
    /// - Response is malformed
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use turbomcp_auth::oauth2::dcr::{DcrClient, DcrBuilder};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = DcrClient::new("https://example.com/register".into(), None);
    /// let request = DcrBuilder::mcp_client("My App", "http://localhost:3000/callback")
    ///     .build();
    ///
    /// let response = client.register(request).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn register(&self, request: RegistrationRequest) -> McpResult<RegistrationResponse> {
        let mut req = self.http_client.post(&self.endpoint).json(&request);

        // Add initial access token if present
        if let Some(ref token) = self.initial_access_token {
            req = req.bearer_auth(token);
        }

        let response = req
            .send()
            .await
            .map_err(|e| McpError::internal(format!("Registration request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(McpError::internal(format!(
                "Registration failed with {}: {}",
                status, body
            )));
        }

        let registration_response = response.json::<RegistrationResponse>().await.map_err(|e| {
            McpError::internal(format!("Failed to parse registration response: {}", e))
        })?;

        Ok(registration_response)
    }

    /// Update an existing client registration (RFC 7592)
    ///
    /// Requires the `registration_access_token` from the original registration.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use turbomcp_auth::oauth2::dcr::{DcrClient, DcrBuilder};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = DcrClient::new("https://example.com/register".into(), None);
    /// # let original_response = client.register(DcrBuilder::mcp_client("App", "http://localhost:3000/callback").build()).await?;
    /// // Update the registration
    /// let updated = DcrBuilder::mcp_client("Updated App Name", "http://localhost:3000/callback")
    ///     .with_scopes(vec!["mcp:tools".to_string(), "mcp:resources".to_string()])
    ///     .build();
    ///
    /// let response = client.update(
    ///     &original_response.registration_client_uri.unwrap(),
    ///     &original_response.registration_access_token.unwrap(),
    ///     updated,
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn update(
        &self,
        registration_uri: &str,
        access_token: &str,
        request: RegistrationRequest,
    ) -> McpResult<RegistrationResponse> {
        let response = self
            .http_client
            .put(registration_uri)
            .bearer_auth(access_token)
            .json(&request)
            .send()
            .await
            .map_err(|e| McpError::internal(format!("Update request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(McpError::internal(format!(
                "Update failed with {}: {}",
                status, body
            )));
        }

        let registration_response = response
            .json::<RegistrationResponse>()
            .await
            .map_err(|e| McpError::internal(format!("Failed to parse update response: {}", e)))?;

        Ok(registration_response)
    }

    /// Delete a client registration (RFC 7592)
    ///
    /// Requires the `registration_access_token` from the original registration.
    pub async fn delete(&self, registration_uri: &str, access_token: &str) -> McpResult<()> {
        let response = self
            .http_client
            .delete(registration_uri)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| McpError::internal(format!("Delete request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(McpError::internal(format!(
                "Delete failed with {}: {}",
                status, body
            )));
        }

        Ok(())
    }
}

/// Builder for dynamic client registration requests
///
/// Provides convenient methods for constructing registration requests
/// with sensible defaults for MCP clients.
///
/// # Example
///
/// ```rust
/// use turbomcp_auth::oauth2::dcr::DcrBuilder;
///
/// let request = DcrBuilder::mcp_client("My MCP Client", "http://localhost:3000/callback")
///     .with_scopes(vec!["mcp:tools".to_string(), "mcp:resources".to_string()])
///     .with_client_uri("https://my-app.example.com".to_string())
///     .with_contacts(vec!["admin@example.com".to_string()])
///     .build();
/// ```
pub struct DcrBuilder {
    request: RegistrationRequest,
}

impl DcrBuilder {
    /// Create a new DCR builder for MCP client
    ///
    /// Sets sensible defaults:
    /// - Grant types: authorization_code, refresh_token
    /// - Response types: code
    /// - Token endpoint auth: client_secret_basic
    /// - Application type: web
    /// - Software ID: turbomcp
    ///
    /// # Arguments
    ///
    /// * `client_name` - Human-readable client name
    /// * `redirect_uri` - OAuth redirect URI
    pub fn mcp_client(client_name: &str, redirect_uri: &str) -> Self {
        Self {
            request: RegistrationRequest {
                client_name: Some(client_name.to_string()),
                redirect_uris: Some(vec![redirect_uri.to_string()]),
                grant_types: Some(vec![
                    "authorization_code".to_string(),
                    "refresh_token".to_string(),
                ]),
                response_types: Some(vec!["code".to_string()]),
                token_endpoint_auth_method: Some("client_secret_basic".to_string()),
                application_type: Some("web".to_string()),
                software_id: Some("turbomcp".to_string()),
                software_version: Some(env!("CARGO_PKG_VERSION").to_string()),
                scope: None,
                client_uri: None,
                logo_uri: None,
                contacts: None,
                tos_uri: None,
                policy_uri: None,
                jwks_uri: None,
            },
        }
    }

    /// Create a builder for a native/mobile client
    ///
    /// Sets application_type to "native" and uses appropriate auth method
    pub fn native_client(client_name: &str, redirect_uri: &str) -> Self {
        let mut builder = Self::mcp_client(client_name, redirect_uri);
        builder.request.application_type = Some("native".to_string());
        builder.request.token_endpoint_auth_method = Some("none".to_string()); // Public client
        builder
    }

    /// Set OAuth scopes
    pub fn with_scopes(mut self, scopes: Vec<String>) -> Self {
        self.request.scope = Some(scopes.join(" "));
        self
    }

    /// Set client homepage URI
    pub fn with_client_uri(mut self, uri: String) -> Self {
        self.request.client_uri = Some(uri);
        self
    }

    /// Set logo URI
    pub fn with_logo_uri(mut self, uri: String) -> Self {
        self.request.logo_uri = Some(uri);
        self
    }

    /// Set contact emails
    pub fn with_contacts(mut self, contacts: Vec<String>) -> Self {
        self.request.contacts = Some(contacts);
        self
    }

    /// Set terms of service URI
    pub fn with_tos_uri(mut self, uri: String) -> Self {
        self.request.tos_uri = Some(uri);
        self
    }

    /// Set privacy policy URI
    pub fn with_policy_uri(mut self, uri: String) -> Self {
        self.request.policy_uri = Some(uri);
        self
    }

    /// Set JWKS URI for public keys
    pub fn with_jwks_uri(mut self, uri: String) -> Self {
        self.request.jwks_uri = Some(uri);
        self
    }

    /// Set additional redirect URIs
    pub fn with_redirect_uris(mut self, uris: Vec<String>) -> Self {
        self.request.redirect_uris = Some(uris);
        self
    }

    /// Build the registration request
    pub fn build(self) -> RegistrationRequest {
        self.request
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dcr_builder_mcp_client() {
        let request = DcrBuilder::mcp_client("My MCP Client", "http://localhost:3000/callback")
            .with_scopes(vec!["mcp:tools".to_string()])
            .build();

        assert_eq!(request.client_name, Some("My MCP Client".to_string()));
        assert_eq!(
            request.redirect_uris,
            Some(vec!["http://localhost:3000/callback".to_string()])
        );
        assert_eq!(request.scope, Some("mcp:tools".to_string()));
        assert!(request.software_id.is_some());
        assert_eq!(request.application_type, Some("web".to_string()));
    }

    #[test]
    fn test_dcr_builder_native_client() {
        let request = DcrBuilder::native_client("My App", "myapp://callback").build();

        assert_eq!(request.application_type, Some("native".to_string()));
        assert_eq!(request.token_endpoint_auth_method, Some("none".to_string()));
    }

    #[test]
    fn test_registration_response_deserialization() {
        let json = r#"{
            "client_id": "s6BhdRkqt3",
            "client_secret": "cf136dc3c1fc93f31185e5885805d",
            "client_secret_expires_at": 1577858400,
            "registration_access_token": "this.is.an.access.token.value.ffx83",
            "registration_client_uri": "https://server.example.com/register/s6BhdRkqt3",
            "client_id_issued_at": 1571158400
        }"#;

        let response: RegistrationResponse = serde_json::from_str(json).unwrap();

        assert_eq!(response.client_id, "s6BhdRkqt3");
        assert_eq!(
            response.client_secret,
            Some("cf136dc3c1fc93f31185e5885805d".to_string())
        );
        assert_eq!(response.client_secret_expires_at, Some(1577858400));
        assert!(response.registration_access_token.is_some());
        assert!(response.registration_client_uri.is_some());
    }

    #[test]
    fn test_dcr_client_creation() {
        let client = DcrClient::new(
            "https://auth.example.com/register".to_string(),
            Some("initial_token".to_string()),
        );

        assert_eq!(client.endpoint, "https://auth.example.com/register");
        assert!(client.initial_access_token.is_some());
    }
}
