//! # Client ID Metadata Document Types
//!
//! Types for OAuth 2.0 Client ID Metadata Documents as defined in:
//! - [draft-ietf-oauth-client-id-metadata-document-00]
//! - MCP 2025-11-25 Specification (SEP-991)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use url::Url;

/// Client metadata document
///
/// This struct represents an OAuth 2.0 client metadata document
/// as specified in draft-ietf-oauth-client-id-metadata-document.
///
/// ## Required Fields
///
/// - `client_id`: MUST be an HTTPS URL that matches the document location
/// - `redirect_uris`: At least one redirect URI MUST be provided
///
/// ## Example
///
/// ```json
/// {
///   "client_id": "https://app.example.com/oauth/client-metadata.json",
///   "client_name": "Example MCP Client",
///   "client_uri": "https://app.example.com",
///   "logo_uri": "https://app.example.com/logo.png",
///   "redirect_uris": [
///     "http://127.0.0.1:3000/callback",
///     "http://localhost:3000/callback"
///   ],
///   "grant_types": ["authorization_code"],
///   "response_types": ["code"],
///   "token_endpoint_auth_method": "none"
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClientMetadata {
    /// Client identifier - MUST be an HTTPS URL
    pub client_id: String,

    /// Human-readable client name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,

    /// URL of the client's home page
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_uri: Option<String>,

    /// URL of the client's logo
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_uri: Option<String>,

    /// Array of redirect URIs
    pub redirect_uris: Vec<String>,

    /// Grant types supported (default: ["authorization_code"])
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant_types: Option<Vec<String>>,

    /// Response types supported (default: ["code"])
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_types: Option<Vec<String>>,

    /// Token endpoint authentication method
    ///
    /// Common values:
    /// - "none" - Public client (no authentication)
    /// - "client_secret_basic" - HTTP Basic authentication
    /// - "client_secret_post" - POST parameter
    /// - "private_key_jwt" - JWT with private key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth_method: Option<String>,

    /// JWK Set URL (for private_key_jwt auth)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwks_uri: Option<String>,

    /// JWK Set (inline keys, alternative to jwks_uri)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwks: Option<serde_json::Value>,

    /// Contacts (email addresses)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contacts: Option<Vec<String>>,

    /// Software ID (identifier for the client software)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub software_id: Option<String>,

    /// Software version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub software_version: Option<String>,

    /// Software statement (JWT from software publisher)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub software_statement: Option<String>,

    /// Scope values (space-separated string)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,

    /// Terms of service URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tos_uri: Option<String>,

    /// Privacy policy URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_uri: Option<String>,

    /// Additional metadata fields
    #[serde(flatten)]
    pub additional_fields: HashMap<String, serde_json::Value>,
}

impl ClientMetadata {
    /// Create a new client metadata document
    pub fn new(client_id: String, redirect_uris: Vec<String>) -> Self {
        Self {
            client_id,
            redirect_uris,
            client_name: None,
            client_uri: None,
            logo_uri: None,
            grant_types: None,
            response_types: None,
            token_endpoint_auth_method: None,
            jwks_uri: None,
            jwks: None,
            contacts: None,
            software_id: None,
            software_version: None,
            software_statement: None,
            scope: None,
            tos_uri: None,
            policy_uri: None,
            additional_fields: HashMap::new(),
        }
    }

    /// Validate the client metadata
    ///
    /// # Errors
    ///
    /// Returns [`ClientMetadataError`] if validation fails
    pub fn validate(&self) -> Result<(), ClientMetadataError> {
        // Validate client_id is HTTPS URL
        let client_id_url = Url::parse(&self.client_id)
            .map_err(|e| ClientMetadataError::InvalidClientId(format!("Invalid URL: {}", e)))?;

        if client_id_url.scheme() != "https" {
            return Err(ClientMetadataError::InvalidClientId(
                "client_id MUST use https scheme".to_string(),
            ));
        }

        // Validate redirect_uris is not empty
        if self.redirect_uris.is_empty() {
            return Err(ClientMetadataError::MissingRedirectUris);
        }

        // Validate each redirect URI is a valid URI
        for uri in &self.redirect_uris {
            Url::parse(uri).map_err(|e| {
                ClientMetadataError::InvalidRedirectUri(format!("Invalid redirect URI: {}", e))
            })?;
        }

        // Validate optional URIs if present
        if let Some(ref client_uri) = self.client_uri {
            Url::parse(client_uri).map_err(|e| {
                ClientMetadataError::InvalidField(format!("Invalid client_uri: {}", e))
            })?;
        }

        if let Some(ref logo_uri) = self.logo_uri {
            Url::parse(logo_uri).map_err(|e| {
                ClientMetadataError::InvalidField(format!("Invalid logo_uri: {}", e))
            })?;
        }

        if let Some(ref jwks_uri) = self.jwks_uri {
            Url::parse(jwks_uri).map_err(|e| {
                ClientMetadataError::InvalidField(format!("Invalid jwks_uri: {}", e))
            })?;
        }

        if let Some(ref tos_uri) = self.tos_uri {
            Url::parse(tos_uri).map_err(|e| {
                ClientMetadataError::InvalidField(format!("Invalid tos_uri: {}", e))
            })?;
        }

        if let Some(ref policy_uri) = self.policy_uri {
            Url::parse(policy_uri).map_err(|e| {
                ClientMetadataError::InvalidField(format!("Invalid policy_uri: {}", e))
            })?;
        }

        Ok(())
    }

    /// Check if this is a public client (no authentication method)
    pub fn is_public_client(&self) -> bool {
        matches!(
            self.token_endpoint_auth_method.as_deref(),
            None | Some("none")
        )
    }

    /// Check if this client uses private_key_jwt authentication
    pub fn uses_private_key_jwt(&self) -> bool {
        self.token_endpoint_auth_method.as_deref() == Some("private_key_jwt")
    }

    /// Get the grant types (defaults to ["authorization_code"])
    pub fn grant_types(&self) -> Vec<String> {
        self.grant_types
            .clone()
            .unwrap_or_else(|| vec!["authorization_code".to_string()])
    }

    /// Get the response types (defaults to ["code"])
    pub fn response_types(&self) -> Vec<String> {
        self.response_types
            .clone()
            .unwrap_or_else(|| vec!["code".to_string()])
    }
}

/// Client metadata validation errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum ClientMetadataError {
    /// Invalid client_id
    #[error("Invalid client_id: {0}")]
    InvalidClientId(String),

    /// Missing redirect_uris
    #[error("redirect_uris is required and cannot be empty")]
    MissingRedirectUris,

    /// Invalid redirect URI
    #[error("Invalid redirect URI: {0}")]
    InvalidRedirectUri(String),

    /// Invalid field
    #[error("Invalid metadata field: {0}")]
    InvalidField(String),

    /// client_id mismatch
    #[error("client_id in document ({document}) does not match URL ({url})")]
    ClientIdMismatch { document: String, url: String },

    /// JSON parsing error
    #[error("Failed to parse metadata: {0}")]
    ParseError(String),
}

/// Wrapper for a validated client metadata document
#[derive(Debug, Clone)]
pub struct ValidatedClientMetadata {
    /// The metadata document
    metadata: ClientMetadata,

    /// The URL from which this metadata was fetched
    source_url: String,

    /// When this metadata was fetched (for cache expiry)
    fetched_at: std::time::SystemTime,
}

impl ValidatedClientMetadata {
    /// Create a new validated metadata document
    ///
    /// # Errors
    ///
    /// Returns [`ClientMetadataError`] if validation fails
    pub fn new(metadata: ClientMetadata, source_url: String) -> Result<Self, ClientMetadataError> {
        // Validate the metadata
        metadata.validate()?;

        // Validate that client_id matches source URL
        if metadata.client_id != source_url {
            return Err(ClientMetadataError::ClientIdMismatch {
                document: metadata.client_id.clone(),
                url: source_url,
            });
        }

        Ok(Self {
            metadata,
            source_url,
            fetched_at: std::time::SystemTime::now(),
        })
    }

    /// Get the metadata
    pub fn metadata(&self) -> &ClientMetadata {
        &self.metadata
    }

    /// Get the source URL
    pub fn source_url(&self) -> &str {
        &self.source_url
    }

    /// Get when this was fetched
    pub fn fetched_at(&self) -> std::time::SystemTime {
        self.fetched_at
    }

    /// Check if a redirect URI is allowed by this metadata
    pub fn is_redirect_uri_allowed(&self, redirect_uri: &str) -> bool {
        self.metadata
            .redirect_uris
            .contains(&redirect_uri.to_string())
    }

    /// Validate a redirect URI against this metadata
    ///
    /// # Errors
    ///
    /// Returns error if redirect URI is not in the allowed list
    pub fn validate_redirect_uri(&self, redirect_uri: &str) -> Result<(), ClientMetadataError> {
        if self.is_redirect_uri_allowed(redirect_uri) {
            Ok(())
        } else {
            Err(ClientMetadataError::InvalidRedirectUri(format!(
                "{} is not an allowed redirect URI for client {}",
                redirect_uri, self.metadata.client_id
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_metadata_creation() {
        let metadata = ClientMetadata::new(
            "https://example.com/client-metadata.json".to_string(),
            vec!["http://localhost:3000/callback".to_string()],
        );

        assert_eq!(
            metadata.client_id,
            "https://example.com/client-metadata.json"
        );
        assert_eq!(metadata.redirect_uris.len(), 1);
    }

    #[test]
    fn test_client_metadata_validation_success() {
        let metadata = ClientMetadata::new(
            "https://example.com/client-metadata.json".to_string(),
            vec!["http://localhost:3000/callback".to_string()],
        );

        assert!(metadata.validate().is_ok());
    }

    #[test]
    fn test_client_metadata_validation_requires_https() {
        let metadata = ClientMetadata::new(
            "http://example.com/client-metadata.json".to_string(),
            vec!["http://localhost:3000/callback".to_string()],
        );

        assert!(matches!(
            metadata.validate(),
            Err(ClientMetadataError::InvalidClientId(_))
        ));
    }

    #[test]
    fn test_client_metadata_validation_requires_redirect_uris() {
        let metadata = ClientMetadata::new(
            "https://example.com/client-metadata.json".to_string(),
            vec![],
        );

        assert!(matches!(
            metadata.validate(),
            Err(ClientMetadataError::MissingRedirectUris)
        ));
    }

    #[test]
    fn test_validated_client_metadata_client_id_match() {
        let metadata = ClientMetadata::new(
            "https://example.com/client-metadata.json".to_string(),
            vec!["http://localhost:3000/callback".to_string()],
        );

        let validated = ValidatedClientMetadata::new(
            metadata,
            "https://example.com/client-metadata.json".to_string(),
        );

        assert!(validated.is_ok());
    }

    #[test]
    fn test_validated_client_metadata_client_id_mismatch() {
        let metadata = ClientMetadata::new(
            "https://example.com/client-metadata.json".to_string(),
            vec!["http://localhost:3000/callback".to_string()],
        );

        let validated =
            ValidatedClientMetadata::new(metadata, "https://attacker.com/fake.json".to_string());

        assert!(matches!(
            validated,
            Err(ClientMetadataError::ClientIdMismatch { .. })
        ));
    }

    #[test]
    fn test_redirect_uri_validation() {
        let metadata = ClientMetadata::new(
            "https://example.com/client-metadata.json".to_string(),
            vec![
                "http://localhost:3000/callback".to_string(),
                "http://127.0.0.1:3000/callback".to_string(),
            ],
        );

        let validated = ValidatedClientMetadata::new(
            metadata,
            "https://example.com/client-metadata.json".to_string(),
        )
        .unwrap();

        assert!(validated.is_redirect_uri_allowed("http://localhost:3000/callback"));
        assert!(validated.is_redirect_uri_allowed("http://127.0.0.1:3000/callback"));
        assert!(!validated.is_redirect_uri_allowed("http://attacker.com/callback"));
    }

    #[test]
    fn test_is_public_client() {
        let mut metadata = ClientMetadata::new(
            "https://example.com/client-metadata.json".to_string(),
            vec!["http://localhost:3000/callback".to_string()],
        );

        // Default (None) is public
        assert!(metadata.is_public_client());

        // Explicit "none" is public
        metadata.token_endpoint_auth_method = Some("none".to_string());
        assert!(metadata.is_public_client());

        // Other methods are not public
        metadata.token_endpoint_auth_method = Some("client_secret_basic".to_string());
        assert!(!metadata.is_public_client());
    }

    #[test]
    fn test_grant_types_defaults() {
        let metadata = ClientMetadata::new(
            "https://example.com/client-metadata.json".to_string(),
            vec!["http://localhost:3000/callback".to_string()],
        );

        assert_eq!(metadata.grant_types(), vec!["authorization_code"]);
    }

    #[test]
    fn test_response_types_defaults() {
        let metadata = ClientMetadata::new(
            "https://example.com/client-metadata.json".to_string(),
            vec!["http://localhost:3000/callback".to_string()],
        );

        assert_eq!(metadata.response_types(), vec!["code"]);
    }

    #[test]
    fn test_serde_roundtrip() {
        let metadata = ClientMetadata {
            client_id: "https://example.com/client-metadata.json".to_string(),
            client_name: Some("Test Client".to_string()),
            client_uri: Some("https://example.com".to_string()),
            logo_uri: Some("https://example.com/logo.png".to_string()),
            redirect_uris: vec!["http://localhost:3000/callback".to_string()],
            grant_types: Some(vec!["authorization_code".to_string()]),
            response_types: Some(vec!["code".to_string()]),
            token_endpoint_auth_method: Some("none".to_string()),
            jwks_uri: None,
            jwks: None,
            contacts: Some(vec!["admin@example.com".to_string()]),
            software_id: Some("test-client-v1".to_string()),
            software_version: Some("1.0.0".to_string()),
            software_statement: None,
            scope: Some("read write".to_string()),
            tos_uri: Some("https://example.com/tos".to_string()),
            policy_uri: Some("https://example.com/privacy".to_string()),
            additional_fields: HashMap::new(),
        };

        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: ClientMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(metadata, deserialized);
    }
}
