//! OAuth 2.0 Token Introspection (RFC 7662)
//!
//! Provides real-time token validation via authorization server introspection endpoint.
//! Complements JWT validation by enabling immediate revocation checking.
//!
//! # Why Token Introspection?
//!
//! JWT signatures cannot be revoked without key rotation. Introspection provides:
//! - Real-time revocation checking
//! - Centralized token state management
//! - Support for opaque tokens (non-JWT)
//!
//! # Example
//!
//! ```rust,no_run
//! use turbomcp_auth::introspection::IntrospectionClient;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = IntrospectionClient::new(
//!     "https://auth.example.com/oauth/introspect".to_string(),
//!     "client_id".to_string(),
//!     Some("client_secret".to_string()),
//! );
//!
//! // Check if token is active
//! let is_active = client.is_token_active("access_token_here").await?;
//!
//! if is_active {
//!     println!("Token is valid");
//! } else {
//!     println!("Token revoked or expired");
//! }
//! # Ok(())
//! # }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use turbomcp_protocol::{Error as McpError, Result as McpResult};

/// Token introspection request per RFC 7662 Section 2.1
#[derive(Clone, Serialize)]
pub struct IntrospectionRequest {
    /// The token to introspect (REQUIRED)
    pub token: String,

    /// Hint about token type (access_token or refresh_token)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_type_hint: Option<String>,
}

// Manual Debug impl to prevent token exposure in logs (Sprint 3.6)
impl std::fmt::Debug for IntrospectionRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IntrospectionRequest")
            .field("token", &"[REDACTED]")
            .field("token_type_hint", &self.token_type_hint)
            .finish()
    }
}

/// Token introspection response per RFC 7662 Section 2.2
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IntrospectionResponse {
    /// Whether the token is currently active (REQUIRED)
    pub active: bool,

    /// Scope(s) associated with the token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,

    /// Client identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Username (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    /// Token type (Bearer, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,

    /// Expiration timestamp (seconds since epoch)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<u64>,

    /// Issued at timestamp (seconds since epoch)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<u64>,

    /// Not before timestamp (seconds since epoch)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nbf: Option<u64>,

    /// Subject
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,

    /// Audience
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aud: Option<serde_json::Value>,

    /// Issuer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,

    /// JWT ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,

    /// Additional fields
    #[serde(flatten)]
    pub additional: HashMap<String, serde_json::Value>,
}

/// Token introspection client
///
/// # Example
///
/// ```rust,no_run
/// use turbomcp_auth::introspection::IntrospectionClient;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let client = IntrospectionClient::new(
///     "https://auth.example.com/oauth/introspect".to_string(),
///     "my_client_id".to_string(),
///     Some("my_client_secret".to_string()),
/// );
///
/// // Full introspection
/// let response = client.introspect("token_here", Some("access_token")).await?;
/// println!("Token active: {}", response.active);
/// println!("Token scopes: {:?}", response.scope);
///
/// // Quick check
/// let is_active = client.is_token_active("token_here").await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct IntrospectionClient {
    /// Introspection endpoint URL
    endpoint: String,

    /// Client ID for authentication
    client_id: String,

    /// Client secret for authentication (if confidential client)
    client_secret: Option<String>,

    /// HTTP client
    http_client: reqwest::Client,
}

// Manual Debug impl to prevent client_secret exposure in logs (Sprint 3.6)
impl std::fmt::Debug for IntrospectionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IntrospectionClient")
            .field("endpoint", &self.endpoint)
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "[REDACTED]"),
            )
            .field("http_client", &"<reqwest::Client>")
            .finish()
    }
}

impl IntrospectionClient {
    /// Create a new introspection client
    ///
    /// # Arguments
    ///
    /// * `endpoint` - Token introspection endpoint URL
    /// * `client_id` - Client ID for authenticating with the introspection endpoint
    /// * `client_secret` - Client secret (None for public clients)
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_auth::introspection::IntrospectionClient;
    ///
    /// // Confidential client
    /// let client = IntrospectionClient::new(
    ///     "https://auth.example.com/introspect".to_string(),
    ///     "client_id".to_string(),
    ///     Some("secret".to_string()),
    /// );
    ///
    /// // Public client
    /// let public_client = IntrospectionClient::new(
    ///     "https://auth.example.com/introspect".to_string(),
    ///     "public_client".to_string(),
    ///     None,
    /// );
    /// ```
    pub fn new(endpoint: String, client_id: String, client_secret: Option<String>) -> Self {
        Self {
            endpoint,
            client_id,
            client_secret,
            http_client: reqwest::Client::new(),
        }
    }

    /// Introspect a token per RFC 7662
    ///
    /// # Arguments
    ///
    /// * `token` - The token to introspect
    /// * `token_type_hint` - Optional hint (e.g., "access_token", "refresh_token")
    ///
    /// # Returns
    ///
    /// IntrospectionResponse indicating if token is active and its metadata
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - HTTP request fails
    /// - Response is malformed
    /// - Authentication fails
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use turbomcp_auth::introspection::IntrospectionClient;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = IntrospectionClient::new("https://example.com".into(), "id".into(), None);
    /// let response = client.introspect("my_token", Some("access_token")).await?;
    ///
    /// if response.active {
    ///     println!("Token is valid");
    ///     println!("Subject: {:?}", response.sub);
    ///     println!("Scopes: {:?}", response.scope);
    /// } else {
    ///     println!("Token is revoked or expired");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn introspect(
        &self,
        token: &str,
        token_type_hint: Option<&str>,
    ) -> McpResult<IntrospectionResponse> {
        let mut form_data = vec![("token", token), ("client_id", &self.client_id)];

        // Add client secret if present
        let secret_storage;
        if let Some(ref secret) = self.client_secret {
            secret_storage = secret.clone();
            form_data.push(("client_secret", &secret_storage));
        }

        // Add token type hint if present
        let hint_storage;
        if let Some(hint) = token_type_hint {
            hint_storage = hint.to_string();
            form_data.push(("token_type_hint", &hint_storage));
        }

        let response = self
            .http_client
            .post(&self.endpoint)
            .form(&form_data)
            .send()
            .await
            .map_err(|e| McpError::internal(format!("Introspection request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(McpError::internal(format!(
                "Introspection endpoint returned {}: {}",
                status, body
            )));
        }

        let introspection_response =
            response
                .json::<IntrospectionResponse>()
                .await
                .map_err(|e| {
                    McpError::internal(format!("Failed to parse introspection response: {}", e))
                })?;

        Ok(introspection_response)
    }

    /// Check if a token is active (convenience method)
    ///
    /// This is a shortcut for `introspect()` that only returns the `active` field.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use turbomcp_auth::introspection::IntrospectionClient;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = IntrospectionClient::new("https://example.com".into(), "id".into(), None);
    /// if client.is_token_active("my_token").await? {
    ///     // Token is valid, proceed
    /// } else {
    ///     // Token is revoked or expired, reject
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn is_token_active(&self, token: &str) -> McpResult<bool> {
        let response = self.introspect(token, Some("access_token")).await?;
        Ok(response.active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_introspection_client_creation() {
        let client = IntrospectionClient::new(
            "https://auth.example.com/introspect".to_string(),
            "client_id".to_string(),
            Some("secret".to_string()),
        );

        assert_eq!(client.endpoint, "https://auth.example.com/introspect");
        assert_eq!(client.client_id, "client_id");
        assert!(client.client_secret.is_some());
    }

    #[test]
    fn test_introspection_response_active() {
        let json = r#"{"active": true, "client_id": "test_client", "scope": "read write"}"#;
        let response: IntrospectionResponse = serde_json::from_str(json).unwrap();

        assert!(response.active);
        assert_eq!(response.client_id, Some("test_client".to_string()));
        assert_eq!(response.scope, Some("read write".to_string()));
    }

    #[test]
    fn test_introspection_response_inactive() {
        let json = r#"{"active": false}"#;
        let response: IntrospectionResponse = serde_json::from_str(json).unwrap();

        assert!(!response.active);
    }

    #[test]
    fn test_introspection_response_full() {
        let json = r#"{
            "active": true,
            "scope": "read write",
            "client_id": "l238j323ds-23ij4",
            "username": "jdoe",
            "token_type": "Bearer",
            "exp": 1419356238,
            "iat": 1419350238,
            "nbf": 1419350238,
            "sub": "Z5O3upPC88QrAjx00dis",
            "aud": "https://protected.example.net/resource",
            "iss": "https://server.example.com/",
            "jti": "JlbmMiOiJBMTI4Q0JDLUhTMjU2In"
        }"#;

        let response: IntrospectionResponse = serde_json::from_str(json).unwrap();

        assert!(response.active);
        assert_eq!(response.username, Some("jdoe".to_string()));
        assert_eq!(response.token_type, Some("Bearer".to_string()));
        assert_eq!(response.exp, Some(1419356238));
        assert_eq!(response.sub, Some("Z5O3upPC88QrAjx00dis".to_string()));
    }
}
