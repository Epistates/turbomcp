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
/// Per the current MCP specification, authentication is **stateless**.
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
    /// Allow custom-scheme redirect URIs (e.g. `com.example.app://callback`).
    ///
    /// The MCP spec requires every redirect URI to be either `localhost`/loopback
    /// or HTTPS (`Communication Security`); custom schemes for native/mobile
    /// apps are an RFC 8252 pattern the MCP spec doesn't itself endorse. This
    /// defaults to `false` — set it explicitly to opt into custom schemes,
    /// understanding that's a deviation from the MCP redirect-URI requirement
    /// (RFC 8252 §7.1 covers the native-app mitigations you'd want alongside it:
    /// PKCE, exact redirect URI matching, and — ideally — app-claimed HTTPS
    /// redirects instead of a custom scheme where the platform supports them).
    #[serde(default)]
    pub allow_custom_scheme_redirect: bool,
}

// Custom serialization for SecretString
// Security: Never serialize secrets in plaintext - use redacted placeholder
fn serialize_secret<S>(secret: &SecretString, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    // Check if secret is empty to provide accurate serialization
    // This check is safe because it doesn't expose the secret value
    let is_empty = secret.expose_secret().is_empty();

    if is_empty {
        serializer.serialize_str("")
    } else {
        serializer.serialize_str("[REDACTED]")
    }
}

// Custom deserialization for SecretString
fn deserialize_secret<'de, D>(deserializer: D) -> Result<SecretString, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = serde::Deserialize::deserialize(deserializer)?;
    Ok(SecretString::new(s.into()))
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
    /// Authorization servers that can issue tokens for this resource
    /// (RECOMMENDED). RFC 9728 §2 defines this as a JSON array —
    /// `authorization_servers`, plural — even for the common single-AS case;
    /// a bare `authorization_server: String` field serializes the wrong
    /// shape and MCP clients parsing per the RFC would reject it.
    pub authorization_servers: Vec<String>,
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
            authorization_servers: vec![self.default_auth_server.clone()],
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
            Err(McpError::invalid_params(format!(
                "Unknown resource: {}",
                resource_uri
            )))
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
    /// Apple Sign In provider (OAuth 2.1 with custom requirements)
    Apple,
    /// Okta enterprise OAuth2 provider
    Okta,
    /// Auth0 identity platform provider
    Auth0,
    /// Keycloak open-source OIDC provider
    Keycloak,
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
