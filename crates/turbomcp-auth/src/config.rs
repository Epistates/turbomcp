//! Authentication Configuration Types
//!
//! This module contains all configuration structures for the TurboMCP authentication system.

use std::collections::HashMap;
use std::sync::Arc;
#[cfg(feature = "dpop")]
use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use turbomcp_protocol::{Error as McpError, Result as McpResult};

// DPoP support (feature-gated)
#[cfg(feature = "dpop")]
use super::dpop::DpopAlgorithm;

/// Authentication configuration
///
/// # MCP Compliance
///
/// Per MCP specification (2025-06-18), authentication is **stateless**.
/// All authentication is token-based with validation on every request.
/// No server-side session state is maintained.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Enable authentication
    pub enabled: bool,
    /// Authentication provider configuration
    pub providers: Vec<AuthProviderConfig>,
    /// Authorization configuration
    pub authorization: AuthorizationConfig,
}

/// Authentication provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthProviderConfig {
    /// Provider name
    pub name: String,
    /// Provider type
    pub provider_type: AuthProviderType,
    /// Provider-specific settings
    pub settings: HashMap<String, serde_json::Value>,
    /// Whether this provider is enabled
    pub enabled: bool,
    /// Priority (lower number = higher priority)
    pub priority: u32,
}

/// Authentication provider types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuthProviderType {
    /// OAuth 2.0 provider
    OAuth2,
    /// API key provider
    ApiKey,
    /// JWT token provider
    Jwt,
    /// Custom authentication provider
    Custom,
}

/// Security levels for OAuth 2.1 flows
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum SecurityLevel {
    /// Standard OAuth 2.1 with PKCE
    #[default]
    Standard,
    /// Enhanced security with DPoP token binding
    Enhanced,
    /// Maximum security with full DPoP
    Maximum,
}

/// DPoP (Demonstration of Proof-of-Possession) configuration
#[cfg(feature = "dpop")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DpopConfig {
    /// Cryptographic algorithm for DPoP proofs
    pub key_algorithm: DpopAlgorithm,
    /// Proof lifetime in seconds (default: 60s per RFC 9449)
    #[serde(default = "default_proof_lifetime")]
    pub proof_lifetime: Duration,
    /// Maximum clock skew tolerance in seconds (default: 300s per RFC 9449)
    #[serde(default = "default_clock_skew")]
    pub clock_skew_tolerance: Duration,
    /// Key storage backend selection
    #[serde(default)]
    pub key_storage: DpopKeyStorageConfig,
}

#[cfg(feature = "dpop")]
fn default_proof_lifetime() -> Duration {
    Duration::from_secs(60)
}

#[cfg(feature = "dpop")]
fn default_clock_skew() -> Duration {
    Duration::from_secs(300)
}

/// DPoP key storage configuration
#[cfg(feature = "dpop")]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum DpopKeyStorageConfig {
    /// In-memory storage (development)
    #[default]
    Memory,
    /// Redis storage (production)
    Redis {
        /// Redis connection URL
        url: String,
    },
    /// HSM storage (high security)
    Hsm {
        /// HSM configuration parameters
        config: serde_json::Value,
    },
}

#[cfg(feature = "dpop")]
impl Default for DpopConfig {
    fn default() -> Self {
        Self {
            key_algorithm: DpopAlgorithm::ES256,
            proof_lifetime: default_proof_lifetime(),
            clock_skew_tolerance: default_clock_skew(),
            key_storage: DpopKeyStorageConfig::default(),
        }
    }
}

/// Authorization configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizationConfig {
    /// Enable role-based access control
    pub rbac_enabled: bool,
    /// Default roles for new users
    pub default_roles: Vec<String>,
    /// Permission inheritance rules
    pub inheritance_rules: HashMap<String, Vec<String>>,
    /// Resource-based permissions
    pub resource_permissions: HashMap<String, Vec<String>>,
}

/// OAuth 2.1 configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuth2Config {
    /// Client ID
    pub client_id: String,
    /// Client secret (stored securely with automatic zeroization on drop)
    #[serde(
        serialize_with = "serialize_secret",
        deserialize_with = "deserialize_secret"
    )]
    pub client_secret: SecretString,
    /// Authorization endpoint
    pub auth_url: String,
    /// Token endpoint
    pub token_url: String,
    /// Token revocation endpoint (RFC 7009) - optional but recommended
    #[serde(default)]
    pub revocation_url: Option<String>,
    /// Redirect URI
    pub redirect_uri: String,
    /// Scopes to request
    pub scopes: Vec<String>,
    /// OAuth 2.1 flow type
    pub flow_type: OAuth2FlowType,
    /// Additional parameters
    pub additional_params: HashMap<String, String>,
    /// Security level for OAuth flow
    #[serde(default)]
    pub security_level: SecurityLevel,
    /// DPoP configuration (when security_level is Enhanced or Maximum)
    #[cfg(feature = "dpop")]
    #[serde(default)]
    pub dpop_config: Option<DpopConfig>,
    /// MCP server canonical URI for Resource Indicators (RFC 8707)
    /// This is the target resource server URI that tokens will be bound to
    #[serde(default)]
    pub mcp_resource_uri: Option<String>,
    /// Automatic Resource Indicator mode - when true, resource parameter
    /// is automatically included in all OAuth flows for MCP compliance
    #[serde(default = "default_auto_resource_indicators")]
    pub auto_resource_indicators: bool,
}

// Custom serialization for SecretString
fn serialize_secret<S>(secret: &SecretString, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(secret.expose_secret())
}

// Custom deserialization for SecretString
fn deserialize_secret<'de, D>(deserializer: D) -> Result<SecretString, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = serde::Deserialize::deserialize(deserializer)?;
    Ok(SecretString::new(s))
}

/// Default auto resource indicators setting (enabled for MCP compliance)
fn default_auto_resource_indicators() -> bool {
    true
}

/// OAuth 2.1 flow types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OAuth2FlowType {
    /// Authorization Code flow
    AuthorizationCode,
    /// Client Credentials flow
    ClientCredentials,
    /// Device Authorization flow
    DeviceCode,
    /// Implicit flow (not recommended)
    Implicit,
}

/// OAuth 2.1 authorization result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuth2AuthResult {
    /// Authorization URL for user
    pub auth_url: String,
    /// State parameter for CSRF protection
    pub state: String,
    /// Code verifier for PKCE
    pub code_verifier: Option<String>,
    /// Device code (for device flow)
    pub device_code: Option<String>,
    /// User code (for device flow)
    pub user_code: Option<String>,
    /// Verification URL (for device flow)
    pub verification_uri: Option<String>,
}

/// Protected Resource Metadata (RFC 9728) for server-side discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectedResourceMetadata {
    /// Resource server identifier (REQUIRED)
    pub resource: String,
    /// Authorization server endpoint (REQUIRED)
    pub authorization_server: String,
    /// Available scopes for this resource (OPTIONAL)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes_supported: Option<Vec<String>>,
    /// Bearer token methods supported (OPTIONAL)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bearer_methods_supported: Option<Vec<BearerTokenMethod>>,
    /// Resource documentation URI (OPTIONAL)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_documentation: Option<String>,
    /// Additional metadata (OPTIONAL)
    #[serde(flatten)]
    pub additional_metadata: HashMap<String, serde_json::Value>,
}

/// Bearer token delivery methods (RFC 9728)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum BearerTokenMethod {
    /// Authorization header (RFC 6750)
    #[default]
    Header,
    /// Query parameter (RFC 6750) - discouraged for security
    Query,
    /// Request body (RFC 6750) - for POST requests only
    Body,
}

/// MCP Server Resource Registry for RFC 9728 compliance
#[derive(Debug, Clone)]
pub struct McpResourceRegistry {
    /// Map of resource URI to metadata
    resources: Arc<RwLock<HashMap<String, ProtectedResourceMetadata>>>,
    /// Default authorization server for new resources
    default_auth_server: String,
    /// Base resource URI for this MCP server
    base_resource_uri: String,
}

impl McpResourceRegistry {
    /// Create a new MCP resource registry
    #[must_use]
    pub fn new(base_resource_uri: String, auth_server: String) -> Self {
        Self {
            resources: Arc::new(RwLock::new(HashMap::new())),
            default_auth_server: auth_server,
            base_resource_uri,
        }
    }

    /// Register a protected resource (RFC 9728)
    pub async fn register_resource(
        &self,
        resource_id: &str,
        scopes: Vec<String>,
        documentation: Option<String>,
    ) -> McpResult<()> {
        let resource_uri = format!(
            "{}/{}",
            self.base_resource_uri.trim_end_matches('/'),
            resource_id
        );

        let metadata = ProtectedResourceMetadata {
            resource: resource_uri.clone(),
            authorization_server: self.default_auth_server.clone(),
            scopes_supported: Some(scopes),
            bearer_methods_supported: Some(vec![
                BearerTokenMethod::Header, // Primary method
                BearerTokenMethod::Body,   // For POST requests
            ]),
            resource_documentation: documentation,
            additional_metadata: HashMap::new(),
        };

        self.resources.write().await.insert(resource_uri, metadata);
        Ok(())
    }

    /// Get metadata for a specific resource
    pub async fn get_resource_metadata(
        &self,
        resource_uri: &str,
    ) -> Option<ProtectedResourceMetadata> {
        self.resources.read().await.get(resource_uri).cloned()
    }

    /// List all registered resources
    pub async fn list_resources(&self) -> Vec<String> {
        self.resources.read().await.keys().cloned().collect()
    }

    /// Generate RFC 9728 compliant metadata for well-known endpoint
    pub async fn generate_well_known_metadata(&self) -> HashMap<String, ProtectedResourceMetadata> {
        self.resources.read().await.clone()
    }

    /// Validate that a token has required scope for resource access
    pub async fn validate_scope_for_resource(
        &self,
        resource_uri: &str,
        token_scopes: &[String],
    ) -> McpResult<bool> {
        if let Some(metadata) = self.get_resource_metadata(resource_uri).await {
            if let Some(required_scopes) = metadata.scopes_supported {
                // Check if token has at least one required scope
                let has_required_scope = required_scopes
                    .iter()
                    .any(|scope| token_scopes.contains(scope));
                Ok(has_required_scope)
            } else {
                // No specific scopes required
                Ok(true)
            }
        } else {
            Err(McpError::validation(format!(
                "Unknown resource: {}",
                resource_uri
            )))
        }
    }
}

/// Dynamic Client Registration Request (RFC 7591)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientRegistrationRequest {
    /// Client metadata - redirect URIs (REQUIRED for authorization code flow)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_uris: Option<Vec<String>>,
    /// Client metadata - response types
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_types: Option<Vec<String>>,
    /// Client metadata - grant types
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant_types: Option<Vec<String>>,
    /// Application type (web, native)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_type: Option<ApplicationType>,
    /// Human-readable client name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,
    /// Client URI for information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_uri: Option<String>,
    /// Logo URI
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_uri: Option<String>,
    /// Scope string with space-delimited scopes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// Contacts (email addresses)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contacts: Option<Vec<String>>,
    /// Terms of service URI
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tos_uri: Option<String>,
    /// Privacy policy URI
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_uri: Option<String>,
    /// Software ID for client
    #[serde(skip_serializing_if = "Option::is_none")]
    pub software_id: Option<String>,
    /// Software version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub software_version: Option<String>,
}

/// Dynamic Client Registration Response (RFC 7591)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientRegistrationResponse {
    /// Unique client identifier (REQUIRED)
    pub client_id: String,
    /// Client secret (OPTIONAL - not provided for public clients)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    /// Registration access token for client configuration endpoint
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registration_access_token: Option<String>,
    /// Client configuration endpoint
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registration_client_uri: Option<String>,
    /// Client ID issued at timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id_issued_at: Option<i64>,
    /// Client secret expires at timestamp (REQUIRED if client_secret provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret_expires_at: Option<i64>,
    /// Confirmed client metadata - redirect URIs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_uris: Option<Vec<String>>,
    /// Confirmed response types
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_types: Option<Vec<String>>,
    /// Confirmed grant types
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant_types: Option<Vec<String>>,
    /// Confirmed application type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_type: Option<ApplicationType>,
    /// Confirmed client name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,
    /// Confirmed scope
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// Application type for OAuth client (RFC 7591)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ApplicationType {
    /// Web application - runs on web server, can keep secrets
    #[default]
    Web,
    /// Native application - mobile/desktop app, cannot keep secrets
    Native,
}

/// Client Registration Error Response (RFC 7591)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientRegistrationError {
    /// Error code
    pub error: ClientRegistrationErrorCode,
    /// Human-readable error description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_description: Option<String>,
}

/// Client Registration Error Codes (RFC 7591)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClientRegistrationErrorCode {
    /// The value of one or more redirect_uris is invalid
    InvalidRedirectUri,
    /// The value of one of the client metadata fields is invalid
    InvalidClientMetadata,
    /// The software statement presented is invalid
    InvalidSoftwareStatement,
    /// The software statement cannot be checked
    UnapprovedSoftwareStatement,
}

/// Dynamic Client Registration Manager for RFC 7591 compliance
#[derive(Debug, Clone)]
pub struct DynamicClientRegistration {
    /// Registration endpoint URL
    registration_endpoint: String,
    /// Default application type for new registrations
    default_application_type: ApplicationType,
    /// Default grant types
    default_grant_types: Vec<String>,
    /// Default response types
    default_response_types: Vec<String>,
    /// HTTP client for registration requests
    client: reqwest::Client,
}

impl DynamicClientRegistration {
    /// Create a new dynamic client registration manager
    #[must_use]
    pub fn new(registration_endpoint: String) -> Self {
        Self {
            registration_endpoint,
            default_application_type: ApplicationType::Web,
            default_grant_types: vec!["authorization_code".to_string()],
            default_response_types: vec!["code".to_string()],
            client: reqwest::Client::new(),
        }
    }

    /// Register a new OAuth client dynamically (RFC 7591)
    pub async fn register_client(
        &self,
        request: ClientRegistrationRequest,
    ) -> McpResult<ClientRegistrationResponse> {
        // Prepare registration request with defaults
        let mut registration_request = request;

        // Apply defaults if not specified
        if registration_request.application_type.is_none() {
            registration_request.application_type = Some(self.default_application_type.clone());
        }
        if registration_request.grant_types.is_none() {
            registration_request.grant_types = Some(self.default_grant_types.clone());
        }
        if registration_request.response_types.is_none() {
            registration_request.response_types = Some(self.default_response_types.clone());
        }

        // Send registration request
        let response = self
            .client
            .post(&self.registration_endpoint)
            .header("Content-Type", "application/json")
            .json(&registration_request)
            .send()
            .await
            .map_err(|e| McpError::validation(format!("Registration request failed: {}", e)))?;

        // Handle response
        if response.status().is_success() {
            let registration_response: ClientRegistrationResponse =
                response.json().await.map_err(|e| {
                    McpError::validation(format!("Invalid registration response: {}", e))
                })?;
            Ok(registration_response)
        } else {
            // Parse error response
            let error_response: ClientRegistrationError = response
                .json()
                .await
                .map_err(|e| McpError::validation(format!("Invalid error response: {}", e)))?;
            Err(McpError::validation(format!(
                "Client registration failed: {} - {}",
                error_response.error as u32,
                error_response.error_description.unwrap_or_default()
            )))
        }
    }

    /// Create a default MCP client registration request
    #[must_use]
    pub fn create_mcp_client_request(
        client_name: &str,
        redirect_uris: Vec<String>,
        mcp_server_uri: &str,
    ) -> ClientRegistrationRequest {
        ClientRegistrationRequest {
            redirect_uris: Some(redirect_uris),
            response_types: Some(vec!["code".to_string()]),
            grant_types: Some(vec!["authorization_code".to_string()]),
            application_type: Some(ApplicationType::Web),
            client_name: Some(format!("MCP Client: {}", client_name)),
            client_uri: Some(mcp_server_uri.to_string()),
            scope: Some(
                "mcp:tools:read mcp:tools:execute mcp:resources:read mcp:prompts:read".to_string(),
            ),
            software_id: Some("turbomcp".to_string()),
            software_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            logo_uri: None,
            contacts: None,
            tos_uri: None,
            policy_uri: None,
        }
    }
}

/// Device authorization response for CLI/IoT flows
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAuthorizationResponse {
    /// Device verification code
    pub device_code: String,
    /// User-friendly verification code
    pub user_code: String,
    /// Verification URI
    pub verification_uri: String,
    /// Complete verification URI (optional)
    pub verification_uri_complete: Option<String>,
    /// Expires in seconds
    pub expires_in: u64,
    /// Polling interval in seconds
    pub interval: u64,
}

/// Provider-specific configuration for handling OAuth quirks
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    /// Provider type (Google, Microsoft, GitHub, etc.)
    pub provider_type: ProviderType,
    /// Custom scopes required by provider
    pub default_scopes: Vec<String>,
    /// Provider-specific token refresh behavior
    pub refresh_behavior: RefreshBehavior,
    /// Custom userinfo endpoint
    pub userinfo_endpoint: Option<String>,
    /// Additional provider-specific parameters
    pub additional_params: HashMap<String, String>,
}

/// OAuth2 provider types with built-in configurations
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderType {
    /// Google OAuth2 provider
    Google,
    /// Microsoft/Azure OAuth2 provider
    Microsoft,
    /// GitHub OAuth2 provider
    GitHub,
    /// GitLab OAuth2 provider
    GitLab,
    /// Generic OAuth2 provider with standard scopes
    Generic,
    /// Custom provider with custom configuration
    Custom(String),
}

/// Token refresh behavior strategies
#[derive(Debug, Clone)]
pub enum RefreshBehavior {
    /// Always refresh tokens before expiration
    Proactive,
    /// Only refresh when token is actually expired
    Reactive,
    /// Custom refresh logic
    Custom,
}
