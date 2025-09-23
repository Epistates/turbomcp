//! Authentication and Authorization system for `TurboMCP` servers
//!
//! This module provides comprehensive OAuth 2.1 MCP compliance and authentication capabilities including:
//!
//! ## OAuth 2.1 MCP Compliance
//! - **RFC 8707 Resource Indicators** - MCP resource URI binding for token scoping
//! - **RFC 9728 Protected Resource Metadata** - Discovery and validation endpoints
//! - **RFC 7591 Dynamic Client Registration** - Runtime client configuration
//! - **PKCE Support** - Enhanced security with Proof Key for Code Exchange
//! - **Multi-Provider Support** - Google, GitHub, Microsoft OAuth 2.0 integration
//!
//! ## Security Features
//! - **Redirect URI Validation** - Prevents open redirect attacks
//! - **Domain Whitelisting** - Environment-based host validation
//! - **Attack Vector Prevention** - Protection against injection and traversal attacks
//! - **Security Levels** - Standard, Enhanced, Maximum security configurations
//!
//! ## Legacy Authentication (Planned)
//! - JWT token validation and generation
//! - API key authentication
//! - Role-based access control (RBAC)
//! - Custom authentication providers
//! - Session management
//! - Token refresh and validation

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::{McpError, McpResult};

// Using battle-tested oauth2 crate for secure OAuth2 implementation
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
    basic::BasicClient, reqwest::async_http_client,
};

// DPoP support (feature-gated)
#[cfg(feature = "dpop")]
use turbomcp_dpop::{DpopAlgorithm, DpopKeyManager, DpopProofGenerator};

/// Authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Enable authentication
    pub enabled: bool,
    /// Authentication provider configuration
    pub providers: Vec<AuthProviderConfig>,
    /// Session configuration
    pub session: SessionConfig,
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

/// Security levels for OAuth 2.0 flows
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SecurityLevel {
    /// Standard OAuth 2.0 with PKCE (existing behavior - no breaking changes)
    Standard,
    /// Enhanced security with DPoP token binding
    Enhanced,
    /// Maximum security with full DPoP + additional features
    Maximum,
}

impl Default for SecurityLevel {
    fn default() -> Self {
        Self::Standard
    }
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DpopKeyStorageConfig {
    /// In-memory storage (development)
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
impl Default for DpopKeyStorageConfig {
    fn default() -> Self {
        Self::Memory
    }
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

/// Session configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    /// Session timeout duration in seconds
    pub timeout_seconds: u64,
    /// Whether to use secure cookies
    pub secure_cookies: bool,
    /// Cookie domain
    pub cookie_domain: Option<String>,
    /// Session storage type
    pub storage: SessionStorageType,
    /// Maximum concurrent sessions per user
    pub max_sessions_per_user: Option<u32>,
}

/// Session storage types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SessionStorageType {
    /// In-memory storage
    Memory,
    /// Redis storage
    Redis,
    /// Database storage
    Database,
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

/// Authentication context containing user information and session data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthContext {
    /// User ID
    pub user_id: String,
    /// User information
    pub user: UserInfo,
    /// User roles
    pub roles: Vec<String>,
    /// User permissions
    pub permissions: Vec<String>,
    /// Session ID
    pub session_id: String,
    /// Token information
    pub token: Option<TokenInfo>,
    /// Authentication provider used
    pub provider: String,
    /// Authentication timestamp
    pub authenticated_at: SystemTime,
    /// Token expiry time
    pub expires_at: Option<SystemTime>,
    /// Additional metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// User information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    /// User ID
    pub id: String,
    /// Username
    pub username: String,
    /// Email address
    pub email: Option<String>,
    /// Display name
    pub display_name: Option<String>,
    /// Avatar URL
    pub avatar_url: Option<String>,
    /// User metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Token information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    /// Access token
    pub access_token: String,
    /// Token type (Bearer, etc.)
    pub token_type: String,
    /// Refresh token
    pub refresh_token: Option<String>,
    /// Token expiry in seconds
    pub expires_in: Option<u64>,
    /// Token scope
    pub scope: Option<String>,
}

/// OAuth 2.0 configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuth2Config {
    /// Client ID
    pub client_id: String,
    /// Client secret
    pub client_secret: String,
    /// Authorization endpoint
    pub auth_url: String,
    /// Token endpoint
    pub token_url: String,
    /// Redirect URI
    pub redirect_uri: String,
    /// Scopes to request
    pub scopes: Vec<String>,
    /// OAuth 2.0 flow type
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

/// Default auto resource indicators setting (enabled for MCP compliance)
fn default_auto_resource_indicators() -> bool {
    true
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BearerTokenMethod {
    /// Authorization header (RFC 6750)
    Header,
    /// Query parameter (RFC 6750) - discouraged for security
    Query,
    /// Request body (RFC 6750) - for POST requests only
    Body,
}

impl Default for BearerTokenMethod {
    fn default() -> Self {
        Self::Header
    }
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
            Err(McpError::InvalidInput(format!(
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ApplicationType {
    /// Web application - runs on web server, can keep secrets
    Web,
    /// Native application - mobile/desktop app, cannot keep secrets
    Native,
}

impl Default for ApplicationType {
    fn default() -> Self {
        Self::Web
    }
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
            .map_err(|e| McpError::InvalidInput(format!("Registration request failed: {}", e)))?;

        // Handle response
        if response.status().is_success() {
            let registration_response: ClientRegistrationResponse =
                response.json().await.map_err(|e| {
                    McpError::InvalidInput(format!("Invalid registration response: {}", e))
                })?;
            Ok(registration_response)
        } else {
            // Parse error response
            let error_response: ClientRegistrationError = response
                .json()
                .await
                .map_err(|e| McpError::InvalidInput(format!("Invalid error response: {}", e)))?;
            Err(McpError::InvalidInput(format!(
                "Client registration failed: {} - {}",
                error_response.error as u32,
                error_response.error_description.unwrap_or_default()
            )))
        }
    }

    /// Create a default MCP client registration request
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

/// OAuth 2.0 flow types
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

/// OAuth 2.0 authorization result
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

/// Authentication provider trait
#[async_trait]
pub trait AuthProvider: Send + Sync + std::fmt::Debug {
    /// Provider name
    fn name(&self) -> &str;

    /// Provider type
    fn provider_type(&self) -> AuthProviderType;

    /// Authenticate user with credentials
    async fn authenticate(&self, credentials: AuthCredentials) -> McpResult<AuthContext>;

    /// Validate existing token/session
    async fn validate_token(&self, token: &str) -> McpResult<AuthContext>;

    /// Refresh access token
    async fn refresh_token(&self, refresh_token: &str) -> McpResult<TokenInfo>;

    /// Revoke token/session
    async fn revoke_token(&self, token: &str) -> McpResult<()>;

    /// Get user information
    async fn get_user_info(&self, token: &str) -> McpResult<UserInfo>;
}

/// Authentication credentials
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthCredentials {
    /// Username and password
    UsernamePassword {
        /// Username
        username: String,
        /// Password
        password: String,
    },
    /// API key
    ApiKey {
        /// API key
        key: String,
    },
    /// OAuth 2.0 authorization code
    OAuth2Code {
        /// Authorization code
        code: String,
        /// State parameter
        state: String,
    },
    /// JWT token
    JwtToken {
        /// JWT token
        token: String,
    },
    /// Custom credentials
    Custom {
        /// Custom credential data
        data: HashMap<String, serde_json::Value>,
    },
}

/// Production-grade OAuth 2.0 authentication provider supporting all modern flows
#[derive(Debug)]
pub struct OAuth2Provider {
    /// Provider name
    name: String,
    /// OAuth 2.0 configuration
    config: OAuth2Config,
    /// Comprehensive OAuth2 client supporting multiple flows
    oauth_client: OAuth2Client,
    /// Secure token storage
    token_storage: Arc<dyn TokenStorage>,
    /// Pending authorization requests with PKCE verifiers
    pending_auths: Arc<RwLock<HashMap<String, PendingAuth>>>,
    /// Protected Resource Registry (RFC 9728) for server-side discovery
    resource_registry: Option<Arc<McpResourceRegistry>>,
    /// Dynamic Client Registration (RFC 7591) manager
    dynamic_registration: Option<Arc<DynamicClientRegistration>>,
    /// DPoP proof generator for enhanced security
    #[cfg(feature = "dpop")]
    #[allow(dead_code)] // Prepared for future DPoP functionality
    dpop_generator: Option<Arc<DpopProofGenerator>>,
}

/// Production-grade OAuth2 client wrapper supporting all modern flows
#[derive(Debug, Clone)]
pub struct OAuth2Client {
    /// Authorization code flow client (most common)
    auth_code_client: BasicClient,
    /// Client credentials client (server-to-server)
    client_credentials_client: Option<BasicClient>,
    /// Device code client (for CLI/IoT applications)
    device_code_client: Option<BasicClient>,
    /// Provider-specific configuration
    provider_config: ProviderConfig,
}

/// Provider-specific configuration for handling OAuth quirks
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    /// Provider type (Google, Microsoft, GitHub, etc.)
    provider_type: ProviderType,
    /// Custom scopes required by provider
    default_scopes: Vec<String>,
    /// Provider-specific token refresh behavior
    refresh_behavior: RefreshBehavior,
    /// Custom userinfo endpoint
    userinfo_endpoint: Option<String>,
    /// Additional provider-specific parameters
    additional_params: HashMap<String, String>,
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

/// Secure token storage abstraction
#[async_trait]
pub trait TokenStorage: Send + Sync + std::fmt::Debug {
    /// Store access token securely
    async fn store_access_token(&self, user_id: &str, token: &AccessToken) -> McpResult<()>;

    /// Retrieve access token
    async fn get_access_token(&self, user_id: &str) -> McpResult<Option<AccessToken>>;

    /// Store refresh token securely (encrypted at rest)
    async fn store_refresh_token(&self, user_id: &str, token: &RefreshToken) -> McpResult<()>;

    /// Retrieve refresh token
    async fn get_refresh_token(&self, user_id: &str) -> McpResult<Option<RefreshToken>>;

    /// Remove all tokens for user (logout)
    async fn revoke_tokens(&self, user_id: &str) -> McpResult<()>;

    /// List all users with stored tokens (for admin)
    async fn list_users(&self) -> McpResult<Vec<String>>;
}

/// Secure access token with metadata
#[derive(Debug, Clone)]
pub struct AccessToken {
    /// The actual token
    token: String,
    /// Token expiration time
    expires_at: Option<SystemTime>,
    /// Token scopes
    scopes: Vec<String>,
    /// Provider metadata
    metadata: HashMap<String, serde_json::Value>,
}

impl AccessToken {
    /// Create a new access token
    pub fn new(
        token: String,
        expires_at: Option<SystemTime>,
        scopes: Vec<String>,
        metadata: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            token,
            expires_at,
            scopes,
            metadata,
        }
    }

    /// Get the token value
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Get the token expiration time
    pub fn expires_at(&self) -> Option<SystemTime> {
        self.expires_at
    }

    /// Get the token scopes
    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }

    /// Get the token metadata
    pub fn metadata(&self) -> &HashMap<String, serde_json::Value> {
        &self.metadata
    }
}

/// Pending OAuth 2.0 authorization with PKCE support
#[derive(Debug)]
struct PendingAuth {
    state: CsrfToken,
    pkce_verifier: PkceCodeVerifier,
    created_at: SystemTime,
    expires_at: SystemTime,
    /// Resource URI for RFC 8707 Resource Indicators
    resource_uri: Option<String>,
}

impl OAuth2Client {
    /// Create a production-grade OAuth2 client supporting all flows
    pub fn new(config: &OAuth2Config, provider_type: ProviderType) -> McpResult<Self> {
        // Validate URLs
        let auth_url = AuthUrl::new(config.auth_url.clone())
            .map_err(|_| McpError::InvalidInput("Invalid authorization URL".to_string()))?;

        let token_url = TokenUrl::new(config.token_url.clone())
            .map_err(|_| McpError::InvalidInput("Invalid token URL".to_string()))?;

        // Enhanced redirect URI validation with comprehensive security checks
        let redirect_url = Self::validate_redirect_uri(&config.redirect_uri)?;

        // Create authorization code flow client (primary)
        let client_secret = if config.client_secret.is_empty() {
            None
        } else {
            Some(ClientSecret::new(config.client_secret.clone()))
        };

        let auth_code_client = BasicClient::new(
            ClientId::new(config.client_id.clone()),
            client_secret.clone(),
            auth_url.clone(),
            Some(token_url.clone()),
        )
        .set_redirect_uri(redirect_url);

        // Create client credentials client if we have a secret (server-to-server)
        let client_credentials_client = if client_secret.is_some() {
            Some(BasicClient::new(
                ClientId::new(config.client_id.clone()),
                client_secret.clone(),
                auth_url.clone(),
                Some(token_url.clone()),
            ))
        } else {
            None
        };

        // Device code client (for CLI/IoT apps) - uses same configuration
        let device_code_client = Some(BasicClient::new(
            ClientId::new(config.client_id.clone()),
            client_secret,
            auth_url,
            Some(token_url),
        ));

        // Provider-specific configuration
        let provider_config = Self::build_provider_config(provider_type);

        Ok(Self {
            auth_code_client,
            client_credentials_client,
            device_code_client,
            provider_config,
        })
    }

    /// Build provider-specific configuration
    fn build_provider_config(provider_type: ProviderType) -> ProviderConfig {
        match provider_type {
            ProviderType::Google => ProviderConfig {
                provider_type,
                default_scopes: vec![
                    "openid".to_string(),
                    "email".to_string(),
                    "profile".to_string(),
                ],
                refresh_behavior: RefreshBehavior::Proactive,
                userinfo_endpoint: Some(
                    "https://www.googleapis.com/oauth2/v2/userinfo".to_string(),
                ),
                additional_params: HashMap::new(),
            },
            ProviderType::Microsoft => ProviderConfig {
                provider_type,
                default_scopes: vec![
                    "openid".to_string(),
                    "profile".to_string(),
                    "email".to_string(),
                    "User.Read".to_string(),
                ],
                refresh_behavior: RefreshBehavior::Proactive,
                userinfo_endpoint: Some("https://graph.microsoft.com/v1.0/me".to_string()),
                additional_params: HashMap::new(),
            },
            ProviderType::GitHub => ProviderConfig {
                provider_type,
                default_scopes: vec!["user:email".to_string(), "read:user".to_string()],
                refresh_behavior: RefreshBehavior::Reactive,
                userinfo_endpoint: Some("https://api.github.com/user".to_string()),
                additional_params: HashMap::new(),
            },
            _ => ProviderConfig {
                provider_type,
                default_scopes: vec!["openid".to_string(), "profile".to_string()],
                refresh_behavior: RefreshBehavior::Proactive,
                userinfo_endpoint: None,
                additional_params: HashMap::new(),
            },
        }
    }

    /// Comprehensive redirect URI validation with production-grade security checks
    /// Prevents open redirect attacks and enforces production security standards
    fn validate_redirect_uri(uri: &str) -> McpResult<RedirectUrl> {
        // 1. Basic URL parsing and structure validation
        let redirect_url = RedirectUrl::new(uri.to_string())
            .map_err(|_| McpError::InvalidInput("Invalid redirect URI format".to_string()))?;

        let url = redirect_url.url();

        // 2. HTTPS enforcement for production environments (except localhost)
        #[cfg(not(debug_assertions))]
        {
            if url.scheme() != "https" {
                // Allow localhost and loopback for development
                if let Some(host) = url.host_str() {
                    if !Self::is_localhost_or_loopback(host) {
                        return Err(McpError::InvalidInput(
                            "Production redirect URIs must use HTTPS (localhost exempted)"
                                .to_string(),
                        ));
                    }
                }
            }
        }

        // 3. Host validation against security whitelist
        if let Some(host) = url.host_str() {
            if !Self::is_allowed_redirect_host(host) {
                return Err(McpError::InvalidInput(format!(
                    "Redirect URI host '{}' not in security whitelist. Configure allowed hosts in your OAuth provider settings.",
                    host
                )));
            }
        } else {
            return Err(McpError::InvalidInput(
                "Redirect URI must have a valid host".to_string(),
            ));
        }

        // 4. Path validation - prevent suspicious paths
        let path = url.path();
        if Self::is_suspicious_redirect_path(path) {
            return Err(McpError::InvalidInput(format!(
                "Redirect URI path '{}' contains suspicious patterns",
                path
            )));
        }

        // 5. Query parameter validation
        if let Some(query) = url.query()
            && Self::contains_suspicious_query_params(query)
        {
            return Err(McpError::InvalidInput(
                "Redirect URI contains suspicious query parameters".to_string(),
            ));
        }

        Ok(redirect_url)
    }

    /// Check if host is localhost or loopback address
    #[allow(dead_code)] // Reserved for future security validation
    fn is_localhost_or_loopback(host: &str) -> bool {
        matches!(host, "localhost" | "127.0.0.1" | "::1" | "0.0.0.0")
    }

    /// Comprehensive redirect host whitelist validation
    /// This is a security-critical function that prevents open redirect attacks
    fn is_allowed_redirect_host(host: &str) -> bool {
        // Default secure whitelist for development and common patterns
        const DEFAULT_ALLOWED_HOSTS: &[&str] = &["localhost", "127.0.0.1", "::1", "0.0.0.0"];

        // Check default allowed hosts first
        if DEFAULT_ALLOWED_HOSTS.contains(&host) {
            return true;
        }

        // Environment-based whitelist for production
        if let Ok(allowed_hosts) = std::env::var("OAUTH_ALLOWED_REDIRECT_HOSTS") {
            let hosts: Vec<&str> = allowed_hosts.split(',').map(str::trim).collect();
            if hosts.contains(&host) {
                return true;
            }
        }

        // Common secure patterns (customize based on your infrastructure)
        // Example: Allow subdomains of your main domain
        if let Ok(main_domain) = std::env::var("OAUTH_MAIN_DOMAIN")
            && (host == main_domain || host.ends_with(&format!(".{}", main_domain)))
        {
            return true;
        }

        // Reject by default for maximum security
        false
    }

    /// Detect suspicious redirect paths that could be used for attacks
    fn is_suspicious_redirect_path(path: &str) -> bool {
        let suspicious_patterns = [
            "../",         // Path traversal
            "//",          // Protocol-relative URLs
            "javascript:", // JavaScript injection
            "data:",       // Data URLs
            "vbscript:",   // VBScript injection
            "file:",       // File protocol
            "%2e%2e",      // URL-encoded path traversal
            "%2f%2f",      // URL-encoded double slash
        ];

        let lower_path = path.to_lowercase();
        suspicious_patterns
            .iter()
            .any(|&pattern| lower_path.contains(pattern))
    }

    /// Check for suspicious query parameters in redirect URIs
    fn contains_suspicious_query_params(query: &str) -> bool {
        let suspicious_params = [
            "javascript:",
            "data:",
            "vbscript:",
            "<script",
            "onload=",
            "onerror=",
        ];

        let lower_query = query.to_lowercase();
        suspicious_params
            .iter()
            .any(|&pattern| lower_query.contains(pattern))
    }
}

impl OAuth2Provider {
    /// Create a production-grade OAuth 2.0 provider with comprehensive flow support
    pub async fn new(
        name: String,
        config: OAuth2Config,
        provider_type: ProviderType,
        token_storage: Arc<dyn TokenStorage>,
    ) -> McpResult<Self> {
        let oauth_client = OAuth2Client::new(&config, provider_type)?;

        // Initialize DPoP generator for enhanced security levels
        #[cfg(feature = "dpop")]
        let dpop_generator = match config.security_level {
            SecurityLevel::Enhanced | SecurityLevel::Maximum => {
                if let Some(dpop_config) = &config.dpop_config {
                    let key_manager = match &dpop_config.key_storage {
                        DpopKeyStorageConfig::Memory => {
                            Arc::new(DpopKeyManager::new_memory().await.map_err(|e| {
                                McpError::Server(turbomcp_server::ServerError::Internal(
                                    e.to_string(),
                                ))
                            })?)
                        }
                        DpopKeyStorageConfig::Redis { url: _url } => {
                            // Redis support requires additional implementation
                            return Err(McpError::Server(turbomcp_server::ServerError::Internal(
                                "Redis DPoP storage not yet implemented".to_string(),
                            )));
                        }
                        DpopKeyStorageConfig::Hsm { config: _config } => {
                            return Err(McpError::Server(turbomcp_server::ServerError::Internal(
                                "HSM support not yet implemented".to_string(),
                            )));
                        }
                    };
                    Some(Arc::new(DpopProofGenerator::new(key_manager)))
                } else {
                    return Err(McpError::Server(turbomcp_server::ServerError::Internal(
                        "DPoP config required for Enhanced/Maximum security levels".to_string(),
                    )));
                }
            }
            SecurityLevel::Standard => None,
        };

        Ok(Self {
            name,
            config,
            oauth_client,
            token_storage,
            pending_auths: Arc::new(RwLock::new(HashMap::new())),
            resource_registry: None, // Can be set later via with_resource_registry()
            dynamic_registration: None, // Can be set later via with_dynamic_registration()
            #[cfg(feature = "dpop")]
            dpop_generator,
        })
    }

    /// RFC 8707 canonical URI validation for Resource Indicators
    fn validate_canonical_resource_uri(&self, uri: &str) -> McpResult<()> {
        use url::Url;

        let parsed = Url::parse(uri)
            .map_err(|e| McpError::InvalidInput(format!("Invalid resource URI: {e}")))?;

        // RFC 8707 requirements
        if parsed.scheme() != "https" && parsed.scheme() != "http" {
            return Err(McpError::InvalidInput(
                "Resource URI must use http or https scheme".to_string(),
            ));
        }

        if parsed.fragment().is_some() {
            return Err(McpError::InvalidInput(
                "Resource URI must not contain fragment".to_string(),
            ));
        }

        // MCP-specific validation for canonical URIs
        if parsed.host_str().is_none() {
            return Err(McpError::InvalidInput(
                "Resource URI must include host".to_string(),
            ));
        }

        // Ensure canonical form (lowercase scheme and host)
        let canonical_scheme = parsed.scheme().to_lowercase();
        let canonical_host = parsed.host_str().unwrap().to_lowercase();

        if parsed.scheme() != canonical_scheme || parsed.host_str().unwrap() != canonical_host {
            return Err(McpError::InvalidInput(
                "Resource URI must use canonical form (lowercase scheme and host)".to_string(),
            ));
        }

        Ok(())
    }

    /// Start MCP-compliant OAuth 2.1 authorization flow with Resource Indicators
    pub async fn start_authorization_with_resource(
        &self,
        resource_uri: &str,
    ) -> McpResult<OAuth2AuthResult> {
        // Validate resource URI format (RFC 8707 compliance)
        self.validate_canonical_resource_uri(resource_uri)?;

        // Generate PKCE code challenge for OAuth 2.1 compliance (always enabled)
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

        // Build authorization request with MCP compliance
        let mut auth_request = self
            .oauth_client
            .auth_code_client
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(pkce_challenge)
            .add_extra_param("resource", resource_uri); // RFC 8707 MANDATORY

        // Add provider-specific scopes
        for scope in &self.oauth_client.provider_config.default_scopes {
            auth_request = auth_request.add_scope(Scope::new(scope.clone()));
        }

        // Add any additional provider-specific parameters
        for (key, value) in &self.oauth_client.provider_config.additional_params {
            auth_request = auth_request.add_extra_param(key, value);
        }

        let (auth_url, csrf_token) = auth_request.url();

        // Store pending authorization with resource binding
        self.pending_auths.write().await.insert(
            csrf_token.secret().clone(),
            PendingAuth {
                state: csrf_token.clone(),
                pkce_verifier,
                created_at: SystemTime::now(),
                expires_at: SystemTime::now() + Duration::from_secs(600), // 10 minutes
                resource_uri: Some(resource_uri.to_string()),             // Track resource binding
            },
        );

        Ok(OAuth2AuthResult {
            auth_url: auth_url.to_string(),
            state: csrf_token.secret().clone(),
            code_verifier: None, // PKCE verifier stored securely in pending_auths
            device_code: None,
            user_code: None,
            verification_uri: None,
        })
    }

    /// Start comprehensive OAuth 2.0 authorization flow with automatic MCP compliance
    pub async fn start_authorization(&self) -> McpResult<OAuth2AuthResult> {
        // Check for MCP resource URI configuration
        if let Some(resource_uri) = &self.config.mcp_resource_uri {
            return self.start_authorization_with_resource(resource_uri).await;
        }

        // If auto_resource_indicators is enabled but no URI configured, error
        if self.config.auto_resource_indicators {
            return Err(McpError::InvalidInput(
                "MCP Resource Indicators enabled but no resource URI configured. \
                 Set mcp_resource_uri in OAuth2Config or call start_authorization_with_resource()"
                    .to_string(),
            ));
        }

        // Fallback to legacy flow (for non-MCP use cases)
        self.start_authorization_legacy().await
    }

    /// Legacy OAuth 2.0 authorization flow (without Resource Indicators)
    async fn start_authorization_legacy(&self) -> McpResult<OAuth2AuthResult> {
        // Generate PKCE code challenge for maximum security (always enabled)
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

        // Build authorization request with comprehensive security
        let mut auth_request = self
            .oauth_client
            .auth_code_client
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(pkce_challenge);

        // Add provider-specific scopes
        for scope in &self.oauth_client.provider_config.default_scopes {
            auth_request = auth_request.add_scope(Scope::new(scope.clone()));
        }

        // Add any additional provider-specific parameters
        for (key, value) in &self.oauth_client.provider_config.additional_params {
            auth_request = auth_request.add_extra_param(key, value);
        }

        let (auth_url, csrf_token) = auth_request.url();

        // Store pending authorization with comprehensive security
        self.pending_auths.write().await.insert(
            csrf_token.secret().clone(),
            PendingAuth {
                state: csrf_token.clone(),
                pkce_verifier,
                created_at: SystemTime::now(),
                expires_at: SystemTime::now() + Duration::from_secs(600), // 10 minutes
                resource_uri: None, // No resource binding for legacy flow
            },
        );

        Ok(OAuth2AuthResult {
            auth_url: auth_url.to_string(),
            state: csrf_token.secret().clone(),
            code_verifier: None, // PKCE verifier stored securely in pending_auths
            device_code: None,
            user_code: None,
            verification_uri: None,
        })
    }

    /// Exchange authorization code for tokens with comprehensive security validation
    pub async fn exchange_code(&self, code: &str, state: &str) -> McpResult<TokenInfo> {
        // Validate state parameter (CSRF protection)
        let pending = {
            let mut pending_auths = self.pending_auths.write().await;
            pending_auths.remove(state).ok_or_else(|| {
                McpError::Unauthorized("Invalid or expired state parameter".to_string())
            })?
        };

        // Validate state hasn't expired
        if SystemTime::now() > pending.expires_at {
            return Err(McpError::Unauthorized(
                "Authorization request expired".to_string(),
            ));
        }

        // Exchange authorization code for access token with PKCE and Resource Indicators
        let mut token_request = self
            .oauth_client
            .auth_code_client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .set_pkce_verifier(pending.pkce_verifier);

        // Add Resource Indicator if present (RFC 8707 MANDATORY for MCP)
        if let Some(resource_uri) = &pending.resource_uri {
            token_request = token_request.add_extra_param("resource", resource_uri);
        }

        let token_response = token_request
            .request_async(async_http_client)
            .await
            .map_err(|e| McpError::Unauthorized(format!("Token exchange failed: {e}")))?;

        // Extract token information with resource binding metadata
        let mut metadata = HashMap::new();
        if let Some(resource_uri) = &pending.resource_uri {
            metadata.insert(
                "resource_uri".to_string(),
                serde_json::Value::String(resource_uri.clone()),
            );
            metadata.insert(
                "audience".to_string(),
                serde_json::Value::String(resource_uri.clone()),
            );
        }

        let access_token = AccessToken {
            token: token_response.access_token().secret().clone(),
            expires_at: token_response
                .expires_in()
                .map(|duration| SystemTime::now() + duration),
            scopes: token_response
                .scopes()
                .map(|scopes| scopes.iter().map(|s| s.to_string()).collect())
                .unwrap_or_default(),
            metadata,
        };

        // Store access token for future use (production-grade token management)
        self.token_storage
            .store_access_token(pending.state.secret(), &access_token)
            .await
            .map_err(|e| McpError::internal(format!("Failed to store token: {}", e)))?;

        // Store refresh token if available
        if let Some(refresh_token) = token_response.refresh_token() {
            self.token_storage
                .store_refresh_token(pending.state.secret(), refresh_token)
                .await
                .map_err(|e| McpError::internal(format!("Failed to store refresh token: {}", e)))?
        }

        Ok(TokenInfo {
            access_token: access_token.token.clone(),
            token_type: "Bearer".to_string(),
            expires_in: token_response.expires_in().map(|d| d.as_secs()),
            refresh_token: token_response.refresh_token().map(|t| t.secret().clone()),
            scope: Some(access_token.scopes.join(" ")),
        })
    }

    /// Client credentials flow for server-to-server authentication
    pub async fn client_credentials_flow(&self) -> McpResult<TokenInfo> {
        let client = self
            .oauth_client
            .client_credentials_client
            .as_ref()
            .ok_or_else(|| {
                McpError::InvalidInput(
                    "Client credentials flow not supported by this provider".to_string(),
                )
            })?;

        let token_response = client
            .exchange_client_credentials()
            .request_async(async_http_client)
            .await
            .map_err(|e| {
                McpError::Unauthorized(format!("Client credentials exchange failed: {e}"))
            })?;

        let access_token = AccessToken {
            token: token_response.access_token().secret().clone(),
            expires_at: token_response
                .expires_in()
                .map(|duration| SystemTime::now() + duration),
            scopes: token_response
                .scopes()
                .map(|scopes| scopes.iter().map(|s| s.to_string()).collect())
                .unwrap_or_default(),
            metadata: HashMap::new(),
        };

        // Store the client credentials token
        self.token_storage
            .store_access_token("client_credentials", &access_token)
            .await
            .map_err(|e| McpError::internal(format!("Failed to store client token: {}", e)))?;

        Ok(TokenInfo {
            access_token: access_token.token.clone(),
            token_type: "Bearer".to_string(),
            expires_in: token_response.expires_in().map(|d| d.as_secs()),
            refresh_token: None, // Client credentials flow doesn't provide refresh tokens
            scope: Some(access_token.scopes.join(" ")),
        })
    }

    /// Device code flow for CLI/IoT applications  
    pub async fn device_code_flow(&self) -> McpResult<DeviceAuthorizationResponse> {
        let client = self
            .oauth_client
            .device_code_client
            .as_ref()
            .ok_or_else(|| {
                McpError::InvalidInput(
                    "Device code flow not supported by this provider".to_string(),
                )
            })?;

        let details: oauth2::DeviceAuthorizationResponse<
            oauth2::EmptyExtraDeviceAuthorizationFields,
        > = client
            .exchange_device_code()
            .map_err(|e| McpError::InvalidInput(format!("Device code configuration error: {e}")))?
            .request_async(async_http_client)
            .await
            .map_err(|e| McpError::InvalidInput(format!("Device code request failed: {e}")))?;

        Ok(DeviceAuthorizationResponse {
            device_code: details.device_code().secret().clone(),
            user_code: details.user_code().secret().clone(),
            verification_uri: details.verification_uri().to_string(),
            verification_uri_complete: details
                .verification_uri_complete()
                .map(|uri| uri.secret().clone()),
            expires_in: details.expires_in().as_secs(),
            interval: details.interval().as_secs(),
        })
    }

    /// Get stored access token for a user
    pub async fn get_stored_token(&self, user_id: &str) -> McpResult<Option<AccessToken>> {
        self.token_storage.get_access_token(user_id).await
    }

    /// Check if a token is expired
    pub fn is_token_expired(&self, token: &AccessToken) -> bool {
        if let Some(expires_at) = token.expires_at {
            SystemTime::now() > expires_at
        } else {
            false // No expiration time means it doesn't expire
        }
    }

    /// Get user info using provider-specific endpoint
    pub async fn get_user_info_with_provider_config(
        &self,
        access_token: &str,
    ) -> McpResult<UserInfo> {
        let provider_config = &self.oauth_client.provider_config;

        if let Some(_userinfo_endpoint) = &provider_config.userinfo_endpoint {
            // Use provider-specific userinfo endpoint (implementation would go here)
            // For now, fall back to standard method
            self.get_user_info(access_token).await
        } else {
            // Fall back to standard method
            self.get_user_info(access_token).await
        }
    }

    /// Determine if token should be refreshed based on provider refresh behavior
    pub fn should_refresh_token(&self, token: &AccessToken) -> bool {
        let provider_config = &self.oauth_client.provider_config;

        match provider_config.refresh_behavior {
            RefreshBehavior::Proactive => {
                // Refresh if token expires within 5 minutes
                if let Some(expires_at) = token.expires_at {
                    let refresh_threshold = SystemTime::now() + Duration::from_secs(300);
                    expires_at <= refresh_threshold
                } else {
                    false
                }
            }
            RefreshBehavior::Reactive => {
                // Only refresh when token is actually expired
                self.is_token_expired(token)
            }
            RefreshBehavior::Custom => {
                // Custom refresh logic would be implemented per provider
                // For now, default to reactive behavior
                self.is_token_expired(token)
            }
        }
    }

    /// Add token metadata for tracking and audit
    pub fn add_token_metadata(&self, token: &mut AccessToken, key: &str, value: serde_json::Value) {
        token.metadata.insert(key.to_string(), value);
    }

    /// Get the OAuth provider type
    pub fn get_provider_type(&self) -> ProviderType {
        self.oauth_client.provider_config.provider_type.clone()
    }

    /// Clean up expired auth sessions
    pub async fn cleanup_expired_sessions(&self) {
        let mut pending_auths = self.pending_auths.write().await;
        let now = SystemTime::now();

        // Remove sessions older than 10 minutes
        let threshold = now - Duration::from_secs(600);

        pending_auths.retain(|_, auth| auth.created_at > threshold);
    }

    /// Refresh an expired access token
    pub async fn refresh_token(&self, user_id: &str) -> McpResult<Option<TokenInfo>> {
        let refresh_token = match self.token_storage.get_refresh_token(user_id).await? {
            Some(token) => token,
            None => return Ok(None),
        };

        let token_response = self
            .oauth_client
            .auth_code_client
            .exchange_refresh_token(&refresh_token)
            .request_async(async_http_client)
            .await
            .map_err(|e| McpError::Unauthorized(format!("Token refresh failed: {e}")))?;

        let access_token = AccessToken {
            token: token_response.access_token().secret().clone(),
            expires_at: token_response
                .expires_in()
                .map(|duration| SystemTime::now() + duration),
            scopes: token_response
                .scopes()
                .map(|scopes| scopes.iter().map(|s| s.to_string()).collect())
                .unwrap_or_default(),
            metadata: HashMap::new(),
        };

        // Update stored token
        self.token_storage
            .store_access_token(user_id, &access_token)
            .await
            .map_err(|e| McpError::internal(format!("Failed to store refreshed token: {}", e)))?;

        Ok(Some(TokenInfo {
            access_token: access_token.token.clone(),
            token_type: "Bearer".to_string(),
            expires_in: token_response.expires_in().map(|d| d.as_secs()),
            refresh_token: token_response.refresh_token().map(|t| t.secret().clone()),
            scope: Some(access_token.scopes.join(" ")),
        }))
    }

    // ============================================================================
    // RFC 9728: Protected Resource Metadata Methods
    // ============================================================================

    /// Configure resource registry for RFC 9728 compliance (builder pattern)
    pub fn with_resource_registry(mut self, registry: Arc<McpResourceRegistry>) -> Self {
        self.resource_registry = Some(registry);
        self
    }

    /// Get the resource registry (if configured)
    pub fn resource_registry(&self) -> Option<&Arc<McpResourceRegistry>> {
        self.resource_registry.as_ref()
    }

    /// Get the OAuth2 configuration
    pub fn config(&self) -> &OAuth2Config {
        &self.config
    }

    /// Register a new protected resource for discovery (RFC 9728)
    pub async fn register_protected_resource(
        &self,
        resource_id: &str,
        scopes: Vec<String>,
        documentation: Option<String>,
    ) -> McpResult<()> {
        if let Some(registry) = &self.resource_registry {
            registry
                .register_resource(resource_id, scopes, documentation)
                .await
        } else {
            Err(McpError::InvalidInput(
                "Resource registry not configured. Use with_resource_registry()".to_string(),
            ))
        }
    }

    /// Generate RFC 9728 Protected Resource Metadata for well-known endpoint
    pub async fn generate_resource_metadata(
        &self,
    ) -> McpResult<HashMap<String, ProtectedResourceMetadata>> {
        if let Some(registry) = &self.resource_registry {
            Ok(registry.generate_well_known_metadata().await)
        } else {
            Err(McpError::InvalidInput(
                "Resource registry not configured. Use with_resource_registry()".to_string(),
            ))
        }
    }

    /// Validate token scopes against protected resource requirements (RFC 9728)
    pub async fn validate_resource_access(
        &self,
        resource_uri: &str,
        token_scopes: &[String],
    ) -> McpResult<bool> {
        if let Some(registry) = &self.resource_registry {
            registry
                .validate_scope_for_resource(resource_uri, token_scopes)
                .await
        } else {
            // Without registry, allow access (backward compatibility)
            Ok(true)
        }
    }

    /// Create default MCP resource registry with common MCP resources
    pub fn create_default_mcp_registry(
        mcp_server_uri: &str,
        auth_server_uri: &str,
    ) -> Arc<McpResourceRegistry> {
        Arc::new(McpResourceRegistry::new(
            mcp_server_uri.to_string(),
            auth_server_uri.to_string(),
        ))
    }

    /// Register standard MCP resources with default scopes
    pub async fn register_standard_mcp_resources(&self) -> McpResult<()> {
        if let Some(registry) = &self.resource_registry {
            // Register core MCP resources
            registry
                .register_resource(
                    "tools",
                    vec![
                        "mcp:tools:read".to_string(),
                        "mcp:tools:execute".to_string(),
                    ],
                    Some("MCP Tool execution and discovery".to_string()),
                )
                .await?;

            registry
                .register_resource(
                    "resources",
                    vec![
                        "mcp:resources:read".to_string(),
                        "mcp:resources:write".to_string(),
                    ],
                    Some("MCP Resource access and management".to_string()),
                )
                .await?;

            registry
                .register_resource(
                    "prompts",
                    vec![
                        "mcp:prompts:read".to_string(),
                        "mcp:prompts:use".to_string(),
                    ],
                    Some("MCP Prompt template access".to_string()),
                )
                .await?;

            Ok(())
        } else {
            Err(McpError::InvalidInput(
                "Resource registry not configured. Use with_resource_registry()".to_string(),
            ))
        }
    }

    // ============================================================================
    // RFC 7591: Dynamic Client Registration Methods
    // ============================================================================

    /// Configure dynamic client registration for RFC 7591 compliance (builder pattern)
    pub fn with_dynamic_registration(
        mut self,
        registration: Arc<DynamicClientRegistration>,
    ) -> Self {
        self.dynamic_registration = Some(registration);
        self
    }

    /// Get the dynamic client registration manager (if configured)
    pub fn dynamic_registration(&self) -> Option<&Arc<DynamicClientRegistration>> {
        self.dynamic_registration.as_ref()
    }

    /// Register a new OAuth client dynamically (RFC 7591)
    pub async fn register_dynamic_client(
        &self,
        request: ClientRegistrationRequest,
    ) -> McpResult<ClientRegistrationResponse> {
        if let Some(registration) = &self.dynamic_registration {
            registration.register_client(request).await
        } else {
            Err(McpError::InvalidInput(
                "Dynamic client registration not configured. Use with_dynamic_registration()"
                    .to_string(),
            ))
        }
    }

    /// Create a complete OAuth provider from dynamic client registration
    pub async fn from_dynamic_registration(
        name: String,
        registration_endpoint: String,
        auth_url: String,
        token_url: String,
        redirect_uri: String,
        mcp_server_uri: String,
        token_storage: Arc<dyn TokenStorage>,
    ) -> McpResult<Self> {
        // Create dynamic registration manager
        let registration = Arc::new(DynamicClientRegistration::new(registration_endpoint));

        // Create registration request for MCP client
        let registration_request = DynamicClientRegistration::create_mcp_client_request(
            &name,
            vec![redirect_uri.clone()],
            &mcp_server_uri,
        );

        // Register client dynamically
        let registration_response = registration.register_client(registration_request).await?;

        // Create OAuth2Config from registration response
        let config = OAuth2Config {
            client_id: registration_response.client_id,
            client_secret: registration_response.client_secret.unwrap_or_default(),
            auth_url,
            token_url,
            redirect_uri,
            scopes: registration_response
                .scope
                .unwrap_or_default()
                .split_whitespace()
                .map(|s| s.to_string())
                .collect(),
            flow_type: OAuth2FlowType::AuthorizationCode,
            additional_params: HashMap::new(),
            security_level: SecurityLevel::Standard,
            mcp_resource_uri: Some(mcp_server_uri.clone()),
            auto_resource_indicators: true,
            #[cfg(feature = "dpop")]
            dpop_config: None,
        };

        // Create OAuth provider
        let provider = Self::new(name, config, ProviderType::Generic, token_storage)
            .await?
            .with_dynamic_registration(registration);

        Ok(provider)
    }

    /// Discover and register with an OAuth authorization server
    pub async fn discover_and_register(
        client_name: &str,
        authorization_server_uri: &str,
        redirect_uri: String,
        mcp_server_uri: String,
        token_storage: Arc<dyn TokenStorage>,
    ) -> McpResult<Self> {
        // Discover authorization server metadata (simplified - in production would use OpenID Connect Discovery)
        let _discovery_url = format!(
            "{}/.well-known/oauth-authorization-server",
            authorization_server_uri.trim_end_matches('/')
        );

        // For now, construct endpoints based on common patterns
        let auth_url = format!(
            "{}/oauth/authorize",
            authorization_server_uri.trim_end_matches('/')
        );
        let token_url = format!(
            "{}/oauth/token",
            authorization_server_uri.trim_end_matches('/')
        );
        let registration_endpoint = format!(
            "{}/oauth/register",
            authorization_server_uri.trim_end_matches('/')
        );

        Self::from_dynamic_registration(
            client_name.to_string(),
            registration_endpoint,
            auth_url,
            token_url,
            redirect_uri,
            mcp_server_uri,
            token_storage,
        )
        .await
    }

    /// Create default MCP-compliant OAuth provider with dynamic registration
    pub async fn create_mcp_compliant_provider(
        client_name: &str,
        authorization_server_uri: &str,
        mcp_server_uri: &str,
        token_storage: Arc<dyn TokenStorage>,
    ) -> McpResult<Self> {
        // Use standard MCP redirect URI pattern
        let redirect_uri = format!("{}/oauth/callback", mcp_server_uri.trim_end_matches('/'));

        let mut provider = Self::discover_and_register(
            client_name,
            authorization_server_uri,
            redirect_uri,
            mcp_server_uri.to_string(),
            token_storage,
        )
        .await?;

        // Configure resource registry for MCP compliance
        let resource_registry =
            Self::create_default_mcp_registry(mcp_server_uri, authorization_server_uri);
        provider = provider.with_resource_registry(resource_registry);

        // Register standard MCP resources
        provider.register_standard_mcp_resources().await?;

        Ok(provider)
    }
}

#[async_trait]
impl AuthProvider for OAuth2Provider {
    fn name(&self) -> &str {
        &self.name
    }

    fn provider_type(&self) -> AuthProviderType {
        AuthProviderType::OAuth2
    }

    async fn authenticate(&self, credentials: AuthCredentials) -> McpResult<AuthContext> {
        match credentials {
            AuthCredentials::OAuth2Code { code, state } => {
                let token_info = self.exchange_code(&code, &state).await?;
                let user_info = self.get_user_info(&token_info.access_token).await?;

                let expires_at = token_info
                    .expires_in
                    .map(|expires_in| SystemTime::now() + Duration::from_secs(expires_in));

                Ok(AuthContext {
                    user_id: user_info.id.clone(),
                    user: user_info,
                    roles: vec!["user".to_string()], // Default role
                    permissions: vec![],
                    session_id: uuid::Uuid::new_v4().to_string(),
                    token: Some(token_info),
                    provider: self.name.clone(),
                    authenticated_at: SystemTime::now(),
                    expires_at,
                    metadata: HashMap::new(),
                })
            }
            _ => Err(McpError::Tool(
                "Invalid credentials for OAuth2 provider".to_string(),
            )),
        }
    }

    async fn validate_token(&self, token: &str) -> McpResult<AuthContext> {
        // Validate token by fetching user info from OAuth provider's userinfo endpoint
        let user_info = self.get_user_info(token).await?;

        Ok(AuthContext {
            user_id: user_info.id.clone(),
            user: user_info,
            roles: vec!["user".to_string()],
            permissions: vec![],
            session_id: uuid::Uuid::new_v4().to_string(),
            token: Some(TokenInfo {
                access_token: token.to_string(),
                token_type: "Bearer".to_string(),
                refresh_token: None,
                expires_in: None,
                scope: None,
            }),
            provider: self.name.clone(),
            authenticated_at: SystemTime::now(),
            expires_at: None,
            metadata: HashMap::new(),
        })
    }

    async fn refresh_token(&self, refresh_token: &str) -> McpResult<TokenInfo> {
        // Use oauth2 crate for secure token refresh
        let token_response = self
            .oauth_client
            .auth_code_client
            .exchange_refresh_token(&RefreshToken::new(refresh_token.to_string()))
            .request_async(async_http_client)
            .await
            .map_err(|e| McpError::Tool(format!("Token refresh failed: {e}")))?;

        Ok(TokenInfo {
            access_token: token_response.access_token().secret().clone(),
            token_type: "Bearer".to_string(),
            expires_in: token_response.expires_in().map(|d| d.as_secs()),
            refresh_token: token_response
                .refresh_token()
                .map(|t| t.secret().clone())
                .or_else(|| Some(refresh_token.to_string())), // Keep existing if no new one
            scope: token_response.scopes().map(|scopes| {
                scopes
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            }),
        })
    }

    async fn revoke_token(&self, _token: &str) -> McpResult<()> {
        // Implementation would revoke the token with the OAuth provider
        Ok(())
    }

    async fn get_user_info(&self, token: &str) -> McpResult<UserInfo> {
        // Current implementation: Direct HTTP call to userinfo endpoint (works for validation)
        // Enhanced oauth2 crate integration can be added for more advanced OAuth features
        // Current approach provides secure token validation via standard HTTP requests
        if token.trim().is_empty() {
            return Err(crate::McpError::Unauthorized("Empty token".to_string()));
        }

        // Construct userinfo endpoint URL based on token URL base
        let base_url = &self.config.token_url;

        let userinfo_url = base_url
            .trim_end_matches("/token")
            .trim_end_matches("/oauth/token");
        let userinfo_endpoint = format!("{userinfo_url}/userinfo");

        // Use reqwest for secure HTTPS OAuth communication (same as oauth2 crate uses internally)
        let client = reqwest::Client::new();
        let response = client
            .get(&userinfo_endpoint)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| crate::McpError::Network(format!("Failed to fetch user info: {e}")))?;

        if !response.status().is_success() {
            return Err(crate::McpError::Unauthorized(
                "Failed to fetch user info".to_string(),
            ));
        }

        let user_data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| crate::McpError::Network(format!("Invalid JSON response: {e}")))?;

        // Extract user information with comprehensive field mapping
        let user_id = user_data
            .get("id")
            .or_else(|| user_data.get("sub"))
            .or_else(|| user_data.get("user_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let username = user_data
            .get("username")
            .or_else(|| user_data.get("preferred_username"))
            .or_else(|| user_data.get("login"))
            .and_then(|v| v.as_str())
            .unwrap_or(&user_id)
            .to_string();
        let email = user_data
            .get("email")
            .and_then(|v| v.as_str())
            .map(std::string::ToString::to_string);
        let display_name = user_data
            .get("name")
            .or_else(|| user_data.get("display_name"))
            .and_then(|v| v.as_str())
            .map(std::string::ToString::to_string);
        let avatar_url = user_data
            .get("picture")
            .or_else(|| user_data.get("avatar_url"))
            .and_then(|v| v.as_str())
            .map(std::string::ToString::to_string);

        Ok(UserInfo {
            id: user_id,
            username,
            email,
            display_name,
            avatar_url,
            metadata: HashMap::new(),
        })
    }
}

/// API Key authentication provider
#[derive(Debug)]
pub struct ApiKeyProvider {
    /// Provider name
    name: String,
    /// Valid API keys with associated user info
    api_keys: Arc<RwLock<HashMap<String, UserInfo>>>,
}

impl ApiKeyProvider {
    /// Create a new API key provider
    #[must_use]
    pub fn new(name: String) -> Self {
        Self {
            name,
            api_keys: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add an API key
    pub async fn add_api_key(&self, key: String, user_info: UserInfo) {
        self.api_keys.write().await.insert(key, user_info);
    }

    /// Remove an API key
    pub async fn remove_api_key(&self, key: &str) -> bool {
        self.api_keys.write().await.remove(key).is_some()
    }

    /// List all API keys (returns keys only, not full info for security)
    pub async fn list_api_keys(&self) -> Vec<String> {
        self.api_keys.read().await.keys().cloned().collect()
    }
}

#[async_trait]
impl AuthProvider for ApiKeyProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn provider_type(&self) -> AuthProviderType {
        AuthProviderType::ApiKey
    }

    async fn authenticate(&self, credentials: AuthCredentials) -> McpResult<AuthContext> {
        match credentials {
            AuthCredentials::ApiKey { key } => {
                let api_keys = self.api_keys.read().await;
                if let Some(user_info) = api_keys.get(&key) {
                    Ok(AuthContext {
                        user_id: user_info.id.clone(),
                        user: user_info.clone(),
                        roles: vec!["api_user".to_string()],
                        permissions: vec!["api_access".to_string()],
                        session_id: uuid::Uuid::new_v4().to_string(),
                        token: Some(TokenInfo {
                            access_token: key,
                            token_type: "ApiKey".to_string(),
                            refresh_token: None,
                            expires_in: None,
                            scope: None,
                        }),
                        provider: self.name.clone(),
                        authenticated_at: SystemTime::now(),
                        expires_at: None,
                        metadata: HashMap::new(),
                    })
                } else {
                    Err(McpError::Tool("Invalid API key".to_string()))
                }
            }
            _ => Err(McpError::Tool(
                "Invalid credentials for API key provider".to_string(),
            )),
        }
    }

    async fn validate_token(&self, token: &str) -> McpResult<AuthContext> {
        self.authenticate(AuthCredentials::ApiKey {
            key: token.to_string(),
        })
        .await
    }

    async fn refresh_token(&self, _refresh_token: &str) -> McpResult<TokenInfo> {
        Err(McpError::Tool(
            "API keys do not support token refresh".to_string(),
        ))
    }

    async fn revoke_token(&self, token: &str) -> McpResult<()> {
        let removed = self.remove_api_key(token).await;
        if removed {
            Ok(())
        } else {
            Err(McpError::Tool("API key not found".to_string()))
        }
    }

    async fn get_user_info(&self, token: &str) -> McpResult<UserInfo> {
        let api_keys = self.api_keys.read().await;
        api_keys
            .get(token)
            .cloned()
            .ok_or_else(|| McpError::Tool("Invalid API key".to_string()))
    }
}

/// Authentication manager
#[derive(Debug)]
pub struct AuthManager {
    /// Authentication configuration
    config: AuthConfig,
    /// Registered authentication providers
    providers: Arc<RwLock<HashMap<String, Arc<dyn AuthProvider>>>>,
    /// Active sessions
    sessions: Arc<RwLock<HashMap<String, AuthContext>>>,
    /// Session cleanup task handle
    _cleanup_handle: Option<tokio::task::JoinHandle<()>>,
}

impl AuthManager {
    /// Create a new authentication manager
    #[must_use]
    pub fn new(config: AuthConfig) -> Self {
        let manager = Self {
            config,
            providers: Arc::new(RwLock::new(HashMap::new())),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            _cleanup_handle: None,
        };

        // Start session cleanup task
        let sessions_clone = manager.sessions.clone();
        let cleanup_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(300)); // 5 minutes
            loop {
                interval.tick().await;
                let now = SystemTime::now();
                let mut sessions = sessions_clone.write().await;
                sessions
                    .retain(|_, context| context.expires_at.is_none_or(|expires| expires > now));
            }
        });

        Self {
            _cleanup_handle: Some(cleanup_handle),
            ..manager
        }
    }

    /// Add an authentication provider
    pub async fn add_provider(&self, provider: Arc<dyn AuthProvider>) {
        let name = provider.name().to_string();
        self.providers.write().await.insert(name, provider);
    }

    /// Remove an authentication provider
    pub async fn remove_provider(&self, name: &str) -> bool {
        self.providers.write().await.remove(name).is_some()
    }

    /// List available providers
    pub async fn list_providers(&self) -> Vec<String> {
        self.providers.read().await.keys().cloned().collect()
    }

    /// Authenticate user with credentials
    pub async fn authenticate(
        &self,
        provider_name: &str,
        credentials: AuthCredentials,
    ) -> McpResult<AuthContext> {
        if !self.config.enabled {
            return Err(McpError::Tool("Authentication is disabled".to_string()));
        }

        let providers = self.providers.read().await;
        let provider = providers
            .get(provider_name)
            .ok_or_else(|| McpError::Tool(format!("Provider '{provider_name}' not found")))?;

        let mut auth_context = provider.authenticate(credentials).await?;

        // Apply default roles if configured
        if auth_context.roles.is_empty() {
            auth_context.roles = self.config.authorization.default_roles.clone();
        }

        // Store session
        let session_id = auth_context.session_id.clone();
        self.sessions
            .write()
            .await
            .insert(session_id, auth_context.clone());

        Ok(auth_context)
    }

    /// Validate token and get authentication context
    pub async fn validate_token(
        &self,
        token: &str,
        provider_name: Option<&str>,
    ) -> McpResult<AuthContext> {
        if !self.config.enabled {
            return Err(McpError::Tool("Authentication is disabled".to_string()));
        }

        let providers = self.providers.read().await;

        if let Some(provider_name) = provider_name {
            let provider = providers
                .get(provider_name)
                .ok_or_else(|| McpError::Tool(format!("Provider '{provider_name}' not found")))?;
            provider.validate_token(token).await
        } else {
            // Try all providers
            for provider in providers.values() {
                if let Ok(context) = provider.validate_token(token).await {
                    return Ok(context);
                }
            }
            Err(McpError::Tool("Token validation failed".to_string()))
        }
    }

    /// Get session by ID
    pub async fn get_session(&self, session_id: &str) -> Option<AuthContext> {
        self.sessions.read().await.get(session_id).cloned()
    }

    /// Revoke session
    pub async fn revoke_session(&self, session_id: &str) -> McpResult<()> {
        let context = self
            .sessions
            .write()
            .await
            .remove(session_id)
            .ok_or_else(|| McpError::Tool("Session not found".to_string()))?;

        // Try to revoke token with provider
        let providers = self.providers.read().await;
        if let Some(provider) = providers.get(&context.provider)
            && let Some(token) = &context.token
        {
            let _ = provider.revoke_token(&token.access_token).await;
        }

        Ok(())
    }

    /// Check if user has permission
    #[must_use]
    pub fn check_permission(&self, context: &AuthContext, permission: &str) -> bool {
        context.permissions.contains(&permission.to_string())
            || context.roles.iter().any(|role| {
                self.config
                    .authorization
                    .inheritance_rules
                    .get(role)
                    .is_some_and(|perms| perms.contains(&permission.to_string()))
            })
    }

    /// Check if user has role
    #[must_use]
    pub fn check_role(&self, context: &AuthContext, role: &str) -> bool {
        context.roles.contains(&role.to_string())
    }
}

// Note: PKCE functionality is handled by the oauth2 crate's built-in
// PkceCodeChallenge::new_random_sha256() method for maximum security

/// Global authentication manager
static GLOBAL_AUTH_MANAGER: once_cell::sync::Lazy<tokio::sync::RwLock<Option<Arc<AuthManager>>>> =
    once_cell::sync::Lazy::new(|| tokio::sync::RwLock::new(None));

/// Set the global authentication manager
pub async fn set_global_auth_manager(manager: Arc<AuthManager>) {
    *GLOBAL_AUTH_MANAGER.write().await = Some(manager);
}

/// Get the global authentication manager
pub async fn global_auth_manager() -> Option<Arc<AuthManager>> {
    GLOBAL_AUTH_MANAGER.read().await.clone()
}

/// Convenience function to check authentication
pub async fn check_auth(token: &str) -> McpResult<AuthContext> {
    if let Some(manager) = global_auth_manager().await {
        manager.validate_token(token, None).await
    } else {
        Err(McpError::Tool(
            "Authentication manager not initialized".to_string(),
        ))
    }
}

/// Authentication middleware trait
#[async_trait]
pub trait AuthMiddleware: Send + Sync {
    /// Extract authentication token from request
    async fn extract_token(&self, headers: &HashMap<String, String>) -> Option<String>;

    /// Handle authentication failure
    async fn handle_auth_failure(&self, error: McpError) -> McpResult<()>;
}

/// Default authentication middleware
pub struct DefaultAuthMiddleware;

#[async_trait]
impl AuthMiddleware for DefaultAuthMiddleware {
    async fn extract_token(&self, headers: &HashMap<String, String>) -> Option<String> {
        // Try Authorization header first
        if let Some(auth_header) = headers
            .get("authorization")
            .or_else(|| headers.get("Authorization"))
        {
            if let Some(token) = auth_header.strip_prefix("Bearer ") {
                return Some(token.to_string());
            }
            if let Some(token) = auth_header.strip_prefix("ApiKey ") {
                return Some(token.to_string());
            }
        }

        // Try X-API-Key header
        if let Some(api_key) = headers
            .get("x-api-key")
            .or_else(|| headers.get("X-API-Key"))
        {
            return Some(api_key.clone());
        }

        None
    }

    async fn handle_auth_failure(&self, error: McpError) -> McpResult<()> {
        tracing::warn!("Authentication failed: {}", error);
        Err(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oauth2_config() {
        let config = OAuth2Config {
            client_id: "test_client".to_string(),
            client_secret: "test_secret".to_string(),
            auth_url: "https://auth.example.com/oauth/authorize".to_string(),
            token_url: "https://auth.example.com/oauth/token".to_string(),
            redirect_uri: "http://localhost:8080/callback".to_string(),
            scopes: vec!["read".to_string(), "write".to_string()],
            flow_type: OAuth2FlowType::AuthorizationCode,
            additional_params: HashMap::new(),
            security_level: SecurityLevel::Standard,
            mcp_resource_uri: None,
            auto_resource_indicators: false,
            #[cfg(feature = "dpop")]
            dpop_config: None,
        };

        assert_eq!(config.client_id, "test_client");
        assert_eq!(config.flow_type, OAuth2FlowType::AuthorizationCode);
    }

    #[test]
    fn test_oauth2_pkce_integration() {
        // Test that oauth2 crate PKCE functionality works as expected
        let (challenge1, _verifier1) = oauth2::PkceCodeChallenge::new_random_sha256();
        let (challenge2, _verifier2) = oauth2::PkceCodeChallenge::new_random_sha256();

        // Each PKCE challenge should be unique
        assert_ne!(challenge1.as_str(), challenge2.as_str());
        assert!(!challenge1.as_str().is_empty());
        assert!(!challenge2.as_str().is_empty());
    }

    #[tokio::test]
    async fn test_api_key_provider() {
        let provider = ApiKeyProvider::new("test_api".to_string());

        let user_info = UserInfo {
            id: "user123".to_string(),
            username: "testuser".to_string(),
            email: Some("test@example.com".to_string()),
            display_name: Some("Test User".to_string()),
            avatar_url: None,
            metadata: HashMap::new(),
        };

        provider
            .add_api_key("test_key_123".to_string(), user_info.clone())
            .await;

        let credentials = AuthCredentials::ApiKey {
            key: "test_key_123".to_string(),
        };

        let auth_result = provider.authenticate(credentials).await;
        assert!(auth_result.is_ok());

        let context = auth_result.unwrap();
        assert_eq!(context.user.username, "testuser");
        assert_eq!(context.provider, "test_api");
    }

    #[tokio::test]
    async fn test_auth_manager() {
        let config = AuthConfig {
            enabled: true,
            providers: vec![],
            session: SessionConfig {
                timeout_seconds: 3600,
                secure_cookies: true,
                cookie_domain: None,
                storage: SessionStorageType::Memory,
                max_sessions_per_user: Some(5),
            },
            authorization: AuthorizationConfig {
                rbac_enabled: true,
                default_roles: vec!["user".to_string()],
                inheritance_rules: HashMap::new(),
                resource_permissions: HashMap::new(),
            },
        };

        let manager = AuthManager::new(config);
        let api_provider = Arc::new(ApiKeyProvider::new("api".to_string()));
        manager.add_provider(api_provider.clone()).await;

        let providers = manager.list_providers().await;
        assert!(providers.contains(&"api".to_string()));
    }
}
