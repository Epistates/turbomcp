//! Server-side authentication and authorization helpers
//!
//! This module provides utilities for MCP servers to handle:
//! - Protected Resource Metadata discovery (RFC 9728)
//! - WWW-Authenticate header generation for 401 responses
//! - Token validation middleware helpers

use std::collections::HashMap;

use serde_json::{json, Value};
use turbomcp_protocol::{Error as McpError, Result as McpResult};

use crate::config::{ProtectedResourceMetadata, BearerTokenMethod};

/// Protected Resource Metadata endpoint builder
///
/// Helps construct RFC 9728 compliant Protected Resource Metadata responses
/// for the `/.well-known/protected-resource` endpoint.
#[derive(Debug, Clone)]
pub struct ProtectedResourceMetadataBuilder {
    /// Base resource URI
    base_resource_uri: String,
    /// Authorization server endpoint
    auth_server: String,
    /// Supported scopes
    scopes: Vec<String>,
    /// Bearer token methods
    bearer_methods: Vec<BearerTokenMethod>,
    /// Resource documentation
    documentation_uri: Option<String>,
}

impl ProtectedResourceMetadataBuilder {
    /// Create a new metadata builder
    pub fn new(base_resource_uri: String, auth_server: String) -> Self {
        Self {
            base_resource_uri,
            auth_server,
            scopes: vec!["openid".to_string(), "profile".to_string()],
            bearer_methods: vec![BearerTokenMethod::Header, BearerTokenMethod::Body],
            documentation_uri: None,
        }
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
            "authorization_server": self.auth_server,
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
            authorization_server: self.auth_server,
            scopes_supported: Some(self.scopes),
            bearer_methods_supported: Some(self.bearer_methods),
            resource_documentation: self.documentation_uri,
            additional_metadata: HashMap::new(),
        }
    }
}

/// WWW-Authenticate header builder for 401 Unauthorized responses
///
/// Implements RFC 9728 Section 5.1 "WWW-Authenticate Response" for indicating
/// the location of Protected Resource Metadata.
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
            return Err(McpError::validation(
                "Authorization header must have format: Bearer <token>".to_string(),
            ));
        }

        if parts[0].to_lowercase() != "bearer" {
            return Err(McpError::validation(
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
            return Err(McpError::validation("Token is empty".to_string()));
        }

        if token.len() < 10 {
            return Err(McpError::validation("Token is too short".to_string()));
        }

        if token.len() > 10000 {
            return Err(McpError::validation("Token is too long".to_string()));
        }

        Ok(())
    }
}

/// Build a 401 Unauthorized JSON response body
pub fn unauthorized_response_body(
    metadata_uri: &str,
    scope: Option<&str>,
) -> Value {
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

        assert_eq!(
            metadata["resource"],
            "https://api.example.com"
        );
        assert_eq!(
            metadata["authorization_server"],
            "https://auth.example.com"
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
}
