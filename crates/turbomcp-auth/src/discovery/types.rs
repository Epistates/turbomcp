//! # Authorization Server Discovery Types
//!
//! Types for OAuth 2.0 Authorization Server Metadata (RFC 8414) and
//! OpenID Connect Discovery 1.0 as required by MCP 2025-11-25 specification.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use url::Url;

/// Authorization server metadata errors
#[derive(Debug, Clone, Error)]
pub enum DiscoveryError {
    /// Invalid issuer URL
    #[error("Invalid issuer URL: {0}")]
    InvalidIssuer(String),

    /// Missing required field
    #[error("Missing required field: {0}")]
    MissingField(String),

    /// Invalid field value
    #[error("Invalid field value for {field}: {reason}")]
    InvalidField { field: String, reason: String },

    /// Issuer mismatch between URL and document
    #[error("Issuer in document ({document}) does not match expected issuer ({expected})")]
    IssuerMismatch { document: String, expected: String },
}

/// OAuth 2.0 Authorization Server Metadata (RFC 8414)
///
/// This struct represents the metadata returned from the
/// `/.well-known/oauth-authorization-server` endpoint.
///
/// ## Required Fields
///
/// - `issuer`: MUST be an HTTPS URL that matches the authorization server's issuer
/// - `authorization_endpoint`: URL of the authorization endpoint
/// - `token_endpoint`: URL of the token endpoint (unless only implicit flow)
/// - `response_types_supported`: List of supported OAuth 2.0 response types
///
/// ## Example
///
/// ```json
/// {
///   "issuer": "https://server.example.com",
///   "authorization_endpoint": "https://server.example.com/authorize",
///   "token_endpoint": "https://server.example.com/token",
///   "jwks_uri": "https://server.example.com/jwks",
///   "response_types_supported": ["code", "token"],
///   "grant_types_supported": ["authorization_code", "implicit"],
///   "token_endpoint_auth_methods_supported": ["client_secret_basic"]
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuthorizationServerMetadata {
    /// REQUIRED. The authorization server's issuer identifier (HTTPS URL)
    pub issuer: String,

    /// REQUIRED. URL of the authorization server's authorization endpoint
    pub authorization_endpoint: String,

    /// URL of the authorization server's token endpoint
    /// REQUIRED unless only the implicit flow is used
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_endpoint: Option<String>,

    /// URL of the authorization server's JWK Set document
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwks_uri: Option<String>,

    /// URL of the authorization server's registration endpoint
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registration_endpoint: Option<String>,

    /// JSON array containing scope values supported
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes_supported: Option<Vec<String>>,

    /// REQUIRED. JSON array containing OAuth 2.0 response_type values
    pub response_types_supported: Vec<String>,

    /// JSON array containing OAuth 2.0 response_mode values
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_modes_supported: Option<Vec<String>>,

    /// JSON array containing OAuth 2.0 grant_type values
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant_types_supported: Option<Vec<String>>,

    /// JSON array containing client authentication methods
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth_methods_supported: Option<Vec<String>>,

    /// JSON array containing JWS signing algorithms
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth_signing_alg_values_supported: Option<Vec<String>>,

    /// URL of service documentation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_documentation: Option<String>,

    /// Languages and scripts supported for UI
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_locales_supported: Option<Vec<String>>,

    /// URL to OP's policy page
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op_policy_uri: Option<String>,

    /// URL to OP's terms of service
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op_tos_uri: Option<String>,

    /// URL of the revocation endpoint (RFC 7009)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revocation_endpoint: Option<String>,

    /// Client authentication methods for revocation endpoint
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revocation_endpoint_auth_methods_supported: Option<Vec<String>>,

    /// URL of the introspection endpoint (RFC 7662)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub introspection_endpoint: Option<String>,

    /// Client authentication methods for introspection endpoint
    #[serde(skip_serializing_if = "Option::is_none")]
    pub introspection_endpoint_auth_methods_supported: Option<Vec<String>>,

    /// PKCE code challenge methods supported (RFC 7636)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_challenge_methods_supported: Option<Vec<String>>,

    /// Additional metadata fields
    #[serde(flatten)]
    pub additional_fields: HashMap<String, serde_json::Value>,
}

impl AuthorizationServerMetadata {
    /// Validate the authorization server metadata
    ///
    /// # Errors
    ///
    /// Returns [`DiscoveryError`] if validation fails
    pub fn validate(&self) -> Result<(), DiscoveryError> {
        // Validate issuer is HTTPS URL
        let issuer_url = Url::parse(&self.issuer)
            .map_err(|e| DiscoveryError::InvalidIssuer(format!("Invalid issuer URL: {}", e)))?;

        if issuer_url.scheme() != "https" {
            return Err(DiscoveryError::InvalidIssuer(
                "Issuer MUST use https scheme".to_string(),
            ));
        }

        // Validate authorization_endpoint is a valid URL
        Url::parse(&self.authorization_endpoint).map_err(|e| DiscoveryError::InvalidField {
            field: "authorization_endpoint".to_string(),
            reason: format!("Invalid URL: {}", e),
        })?;

        // Validate token_endpoint if present
        if let Some(ref token_endpoint) = self.token_endpoint {
            Url::parse(token_endpoint).map_err(|e| DiscoveryError::InvalidField {
                field: "token_endpoint".to_string(),
                reason: format!("Invalid URL: {}", e),
            })?;
        }

        // Validate jwks_uri if present
        if let Some(ref jwks_uri) = self.jwks_uri {
            Url::parse(jwks_uri).map_err(|e| DiscoveryError::InvalidField {
                field: "jwks_uri".to_string(),
                reason: format!("Invalid URL: {}", e),
            })?;
        }

        // Validate response_types_supported is not empty
        if self.response_types_supported.is_empty() {
            return Err(DiscoveryError::MissingField(
                "response_types_supported cannot be empty".to_string(),
            ));
        }

        Ok(())
    }

    /// Get the grant types (defaults to ["authorization_code", "implicit"])
    pub fn grant_types(&self) -> Vec<String> {
        self.grant_types_supported
            .clone()
            .unwrap_or_else(|| vec!["authorization_code".to_string(), "implicit".to_string()])
    }

    /// Check if PKCE is supported
    pub fn supports_pkce(&self) -> bool {
        self.code_challenge_methods_supported
            .as_ref()
            .map(|methods| !methods.is_empty())
            .unwrap_or(false)
    }

    /// Check if a specific PKCE method is supported
    pub fn supports_pkce_method(&self, method: &str) -> bool {
        self.code_challenge_methods_supported
            .as_ref()
            .map(|methods| methods.iter().any(|m| m == method))
            .unwrap_or(false)
    }
}

/// OpenID Connect Provider Metadata (OpenID Connect Discovery 1.0)
///
/// This struct represents the metadata returned from the
/// `/.well-known/openid-configuration` endpoint.
///
/// Extends OAuth 2.0 Authorization Server Metadata with OIDC-specific fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OIDCProviderMetadata {
    /// OAuth 2.0 base metadata
    #[serde(flatten)]
    pub oauth2: AuthorizationServerMetadata,

    /// REQUIRED. URL of the OP's UserInfo endpoint
    pub userinfo_endpoint: String,

    /// RECOMMENDED. JSON array of supported acr values
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acr_values_supported: Option<Vec<String>>,

    /// REQUIRED. JSON array of supported subject identifier types
    pub subject_types_supported: Vec<String>,

    /// REQUIRED. JSON array of JWS signing algorithms for ID Tokens
    pub id_token_signing_alg_values_supported: Vec<String>,

    /// JSON array of JWE encryption algorithms for ID Tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_token_encryption_alg_values_supported: Option<Vec<String>>,

    /// JSON array of JWE encryption encodings for ID Tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_token_encryption_enc_values_supported: Option<Vec<String>>,

    /// JSON array of JWS signing algorithms for UserInfo responses
    #[serde(skip_serializing_if = "Option::is_none")]
    pub userinfo_signing_alg_values_supported: Option<Vec<String>>,

    /// JSON array of JWE encryption algorithms for UserInfo responses
    #[serde(skip_serializing_if = "Option::is_none")]
    pub userinfo_encryption_alg_values_supported: Option<Vec<String>>,

    /// JSON array of JWE encryption encodings for UserInfo responses
    #[serde(skip_serializing_if = "Option::is_none")]
    pub userinfo_encryption_enc_values_supported: Option<Vec<String>>,

    /// JSON array of JWS signing algorithms for Request Objects
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_object_signing_alg_values_supported: Option<Vec<String>>,

    /// JSON array of display parameter values
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_values_supported: Option<Vec<String>>,

    /// JSON array of claim types
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_types_supported: Option<Vec<String>>,

    /// RECOMMENDED. JSON array of supported claim names
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claims_supported: Option<Vec<String>>,

    /// Boolean indicating if claims parameter is supported
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claims_parameter_supported: Option<bool>,

    /// Boolean indicating if request parameter is supported
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_parameter_supported: Option<bool>,

    /// Boolean indicating if request_uri parameter is supported
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_uri_parameter_supported: Option<bool>,

    /// Boolean indicating if request_uri pre-registration is required
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_request_uri_registration: Option<bool>,
}

impl OIDCProviderMetadata {
    /// Validate the OIDC provider metadata
    ///
    /// # Errors
    ///
    /// Returns [`DiscoveryError`] if validation fails
    pub fn validate(&self) -> Result<(), DiscoveryError> {
        // Validate base OAuth2 metadata
        self.oauth2.validate()?;

        // Validate userinfo_endpoint
        Url::parse(&self.userinfo_endpoint).map_err(|e| DiscoveryError::InvalidField {
            field: "userinfo_endpoint".to_string(),
            reason: format!("Invalid URL: {}", e),
        })?;

        // Validate subject_types_supported is not empty
        if self.subject_types_supported.is_empty() {
            return Err(DiscoveryError::MissingField(
                "subject_types_supported cannot be empty".to_string(),
            ));
        }

        // Validate id_token_signing_alg_values_supported is not empty
        if self.id_token_signing_alg_values_supported.is_empty() {
            return Err(DiscoveryError::MissingField(
                "id_token_signing_alg_values_supported cannot be empty".to_string(),
            ));
        }

        Ok(())
    }
}

/// Validated discovery metadata wrapper
#[derive(Debug, Clone)]
pub struct ValidatedDiscoveryMetadata {
    /// The metadata (either OAuth2 or OIDC)
    metadata: DiscoveryMetadata,

    /// The issuer URL that was used for discovery
    issuer: String,

    /// When this metadata was fetched
    fetched_at: std::time::SystemTime,
}

/// Discovery metadata enum (OAuth2 or OIDC)
///
/// Both variants are boxed to keep enum size small
#[derive(Debug, Clone)]
pub enum DiscoveryMetadata {
    /// OAuth 2.0 Authorization Server Metadata (RFC 8414)
    OAuth2(Box<AuthorizationServerMetadata>),

    /// OpenID Connect Provider Metadata
    OIDC(Box<OIDCProviderMetadata>),
}

impl ValidatedDiscoveryMetadata {
    /// Create new validated OAuth2 metadata
    ///
    /// # Errors
    ///
    /// Returns [`DiscoveryError`] if validation fails
    pub fn new_oauth2(
        metadata: AuthorizationServerMetadata,
        issuer: String,
    ) -> Result<Self, DiscoveryError> {
        metadata.validate()?;

        // Validate issuer matches metadata
        if metadata.issuer != issuer {
            return Err(DiscoveryError::IssuerMismatch {
                document: metadata.issuer.clone(),
                expected: issuer,
            });
        }

        Ok(Self {
            metadata: DiscoveryMetadata::OAuth2(Box::new(metadata)),
            issuer,
            fetched_at: std::time::SystemTime::now(),
        })
    }

    /// Create new validated OIDC metadata
    ///
    /// # Errors
    ///
    /// Returns [`DiscoveryError`] if validation fails
    pub fn new_oidc(
        metadata: OIDCProviderMetadata,
        issuer: String,
    ) -> Result<Self, DiscoveryError> {
        metadata.validate()?;

        // Validate issuer matches metadata
        if metadata.oauth2.issuer != issuer {
            return Err(DiscoveryError::IssuerMismatch {
                document: metadata.oauth2.issuer.clone(),
                expected: issuer,
            });
        }

        Ok(Self {
            metadata: DiscoveryMetadata::OIDC(Box::new(metadata)),
            issuer,
            fetched_at: std::time::SystemTime::now(),
        })
    }

    /// Get the metadata
    pub fn metadata(&self) -> &DiscoveryMetadata {
        &self.metadata
    }

    /// Get the issuer
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// Get when this was fetched
    pub fn fetched_at(&self) -> std::time::SystemTime {
        self.fetched_at
    }

    /// Get the OAuth2 metadata (returns OAuth2 part for both types)
    pub fn oauth2(&self) -> &AuthorizationServerMetadata {
        match &self.metadata {
            DiscoveryMetadata::OAuth2(oauth2) => oauth2,
            DiscoveryMetadata::OIDC(oidc) => &oidc.oauth2,
        }
    }

    /// Get the OIDC metadata if available
    pub fn oidc(&self) -> Option<&OIDCProviderMetadata> {
        match &self.metadata {
            DiscoveryMetadata::OAuth2(_) => None,
            DiscoveryMetadata::OIDC(oidc) => Some(oidc),
        }
    }

    /// Check if this is OIDC metadata
    pub fn is_oidc(&self) -> bool {
        matches!(self.metadata, DiscoveryMetadata::OIDC(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oauth2_metadata_validation_success() {
        let metadata = AuthorizationServerMetadata {
            issuer: "https://server.example.com".to_string(),
            authorization_endpoint: "https://server.example.com/authorize".to_string(),
            token_endpoint: Some("https://server.example.com/token".to_string()),
            jwks_uri: Some("https://server.example.com/jwks".to_string()),
            registration_endpoint: None,
            scopes_supported: Some(vec!["openid".to_string(), "profile".to_string()]),
            response_types_supported: vec!["code".to_string()],
            response_modes_supported: None,
            grant_types_supported: Some(vec!["authorization_code".to_string()]),
            token_endpoint_auth_methods_supported: Some(vec!["client_secret_basic".to_string()]),
            token_endpoint_auth_signing_alg_values_supported: None,
            service_documentation: None,
            ui_locales_supported: None,
            op_policy_uri: None,
            op_tos_uri: None,
            revocation_endpoint: None,
            revocation_endpoint_auth_methods_supported: None,
            introspection_endpoint: None,
            introspection_endpoint_auth_methods_supported: None,
            code_challenge_methods_supported: Some(vec!["S256".to_string()]),
            additional_fields: HashMap::new(),
        };

        assert!(metadata.validate().is_ok());
    }

    #[test]
    fn test_oauth2_metadata_validation_requires_https() {
        let metadata = AuthorizationServerMetadata {
            issuer: "http://server.example.com".to_string(),
            authorization_endpoint: "https://server.example.com/authorize".to_string(),
            token_endpoint: Some("https://server.example.com/token".to_string()),
            jwks_uri: None,
            registration_endpoint: None,
            scopes_supported: None,
            response_types_supported: vec!["code".to_string()],
            response_modes_supported: None,
            grant_types_supported: None,
            token_endpoint_auth_methods_supported: None,
            token_endpoint_auth_signing_alg_values_supported: None,
            service_documentation: None,
            ui_locales_supported: None,
            op_policy_uri: None,
            op_tos_uri: None,
            revocation_endpoint: None,
            revocation_endpoint_auth_methods_supported: None,
            introspection_endpoint: None,
            introspection_endpoint_auth_methods_supported: None,
            code_challenge_methods_supported: None,
            additional_fields: HashMap::new(),
        };

        assert!(matches!(
            metadata.validate(),
            Err(DiscoveryError::InvalidIssuer(_))
        ));
    }

    #[test]
    fn test_pkce_support_detection() {
        let mut metadata = AuthorizationServerMetadata {
            issuer: "https://server.example.com".to_string(),
            authorization_endpoint: "https://server.example.com/authorize".to_string(),
            token_endpoint: Some("https://server.example.com/token".to_string()),
            jwks_uri: None,
            registration_endpoint: None,
            scopes_supported: None,
            response_types_supported: vec!["code".to_string()],
            response_modes_supported: None,
            grant_types_supported: None,
            token_endpoint_auth_methods_supported: None,
            token_endpoint_auth_signing_alg_values_supported: None,
            service_documentation: None,
            ui_locales_supported: None,
            op_policy_uri: None,
            op_tos_uri: None,
            revocation_endpoint: None,
            revocation_endpoint_auth_methods_supported: None,
            introspection_endpoint: None,
            introspection_endpoint_auth_methods_supported: None,
            code_challenge_methods_supported: None,
            additional_fields: HashMap::new(),
        };

        // No PKCE support
        assert!(!metadata.supports_pkce());
        assert!(!metadata.supports_pkce_method("S256"));

        // With PKCE support
        metadata.code_challenge_methods_supported = Some(vec!["S256".to_string()]);
        assert!(metadata.supports_pkce());
        assert!(metadata.supports_pkce_method("S256"));
        assert!(!metadata.supports_pkce_method("plain"));
    }

    #[test]
    fn test_validated_metadata_issuer_match() {
        let metadata = AuthorizationServerMetadata {
            issuer: "https://server.example.com".to_string(),
            authorization_endpoint: "https://server.example.com/authorize".to_string(),
            token_endpoint: Some("https://server.example.com/token".to_string()),
            jwks_uri: None,
            registration_endpoint: None,
            scopes_supported: None,
            response_types_supported: vec!["code".to_string()],
            response_modes_supported: None,
            grant_types_supported: None,
            token_endpoint_auth_methods_supported: None,
            token_endpoint_auth_signing_alg_values_supported: None,
            service_documentation: None,
            ui_locales_supported: None,
            op_policy_uri: None,
            op_tos_uri: None,
            revocation_endpoint: None,
            revocation_endpoint_auth_methods_supported: None,
            introspection_endpoint: None,
            introspection_endpoint_auth_methods_supported: None,
            code_challenge_methods_supported: None,
            additional_fields: HashMap::new(),
        };

        let validated = ValidatedDiscoveryMetadata::new_oauth2(
            metadata,
            "https://server.example.com".to_string(),
        );
        assert!(validated.is_ok());
    }

    #[test]
    fn test_validated_metadata_issuer_mismatch() {
        let metadata = AuthorizationServerMetadata {
            issuer: "https://server.example.com".to_string(),
            authorization_endpoint: "https://server.example.com/authorize".to_string(),
            token_endpoint: Some("https://server.example.com/token".to_string()),
            jwks_uri: None,
            registration_endpoint: None,
            scopes_supported: None,
            response_types_supported: vec!["code".to_string()],
            response_modes_supported: None,
            grant_types_supported: None,
            token_endpoint_auth_methods_supported: None,
            token_endpoint_auth_signing_alg_values_supported: None,
            service_documentation: None,
            ui_locales_supported: None,
            op_policy_uri: None,
            op_tos_uri: None,
            revocation_endpoint: None,
            revocation_endpoint_auth_methods_supported: None,
            introspection_endpoint: None,
            introspection_endpoint_auth_methods_supported: None,
            code_challenge_methods_supported: None,
            additional_fields: HashMap::new(),
        };

        let validated =
            ValidatedDiscoveryMetadata::new_oauth2(metadata, "https://attacker.com".to_string());
        assert!(matches!(
            validated,
            Err(DiscoveryError::IssuerMismatch { .. })
        ));
    }
}
