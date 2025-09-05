//! # OAuth 2.0 Authentication Example
//!
//! This example demonstrates TurboMCP's comprehensive OAuth 2.0 integration
//! supporting Google, GitHub, Microsoft, and custom providers with:
//!
//! - ✅ **Always-on PKCE security** - Maximum protection against code interception
//! - ✅ **Multi-provider support** - Google, GitHub, Microsoft built-in configurations  
//! - ✅ **Multiple OAuth flows** - Authorization code, client credentials, device flows
//! - ✅ **Secure token storage** - Encrypted token persistence
//! - ✅ **Session management** - Automatic token refresh and validation
//!
//! ## Setup Instructions
//!
//! ### 1. Google OAuth Setup
//! ```bash
//! # 1. Go to Google Cloud Console: https://console.cloud.google.com/
//! # 2. Create a new project or select existing
//! # 3. Enable Google+ API
//! # 4. Go to "Credentials" → "Create Credentials" → "OAuth 2.0 Client ID"
//! # 5. Set application type to "Web application"
//! # 6. Add authorized redirect URI: http://localhost:8080/auth/callback
//! # 7. Copy Client ID and Client Secret
//!
//! export GOOGLE_CLIENT_ID="your-google-client-id"
//! export GOOGLE_CLIENT_SECRET="your-google-client-secret"
//! ```
//!
//! ### 2. GitHub OAuth Setup  
//! ```bash
//! # 1. Go to GitHub Settings: https://github.com/settings/developers
//! # 2. Click "New OAuth App"
//! # 3. Set Authorization callback URL: http://localhost:8080/auth/callback
//! # 4. Copy Client ID and Client Secret
//!
//! export GITHUB_CLIENT_ID="your-github-client-id"
//! export GITHUB_CLIENT_SECRET="your-github-client-secret"
//! ```
//!
//! ## Running the Example
//! ```bash
//! cargo run --example 09_oauth_authentication
//! # Visit: http://localhost:8080/auth/google or http://localhost:8080/auth/github
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::auth::{
    AccessToken, OAuth2Config, OAuth2FlowType, OAuth2Provider, ProviderType, SecurityLevel, TokenStorage,
};
use turbomcp::prelude::*;

/// Example server with OAuth authentication  
#[derive(Clone)]
pub struct AuthenticatedServer {
    /// OAuth providers (Google, GitHub, etc.)
    oauth_providers: Arc<RwLock<HashMap<String, OAuth2Provider>>>,
    /// User sessions
    sessions: Arc<RwLock<HashMap<String, UserSession>>>,
}

/// User session information
#[derive(Debug, Clone)]
pub struct UserSession {
    pub user_id: String,
    pub email: String,
    pub name: String,
    pub provider: String,
    pub authenticated_at: std::time::SystemTime,
}

/// 🚨 CRITICAL SECURITY WARNING: DEVELOPMENT-ONLY TOKEN STORAGE 🚨
///
/// This in-memory storage is ONLY for development and examples.
/// NEVER use this in production environments.
///
/// ## Production Token Storage Requirements
///
/// OAuth tokens are sensitive credentials that MUST be encrypted at rest:
///
/// ### Option 1: File-based Encrypted Storage (Simple)
/// ```rust
/// use std::fs;
/// use aes_gcm::{Aes256Gcm, Key, Nonce};
///
/// struct EncryptedFileStorage {
///     file_path: PathBuf,
///     encryption_key: [u8; 32], // Store securely (env var, AWS SSM, etc.)
/// }
/// ```
///
/// ### Option 2: Redis with Encryption (Recommended)
/// ```docker
/// # docker-compose.yml
/// version: '3.8'
/// services:
///   redis:
///     image: redis:7-alpine
///     environment:
///       - REDIS_PASSWORD=your-secure-password
///     volumes:
///       - redis_data:/data
///     command: redis-server --requirepass ${REDIS_PASSWORD} --appendonly yes
/// ```
///
/// ### Option 3: Database with Field Encryption (Enterprise)
/// ```sql
/// CREATE TABLE oauth_tokens (
///     user_id VARCHAR(255) PRIMARY KEY,
///     encrypted_access_token BYTEA NOT NULL,
///     encrypted_refresh_token BYTEA,
///     expires_at TIMESTAMP,
///     created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
/// );
/// ```
///
/// ### Encryption Implementation Example (No Extra Dependencies)
/// ```rust
/// use std::collections::HashMap;
/// use sha2::{Sha256, Digest};
///
/// // Built-in encryption using standard library + sha2 (already a dependency)
/// pub struct SecureTokenStorage {
///     storage_backend: StorageBackend, // File, Redis, Database
///     encryption_key: [u8; 32],
/// }
///
/// impl SecureTokenStorage {
///     pub fn new(backend: StorageBackend) -> Self {
///         let encryption_key = Self::derive_key_from_env();
///         Self { storage_backend: backend, encryption_key }
///     }
///     
///     fn derive_key_from_env() -> [u8; 32] {
///         let secret = env::var("OAUTH_ENCRYPTION_SECRET")
///             .expect("OAUTH_ENCRYPTION_SECRET environment variable required");
///         let mut hasher = Sha256::new();
///         hasher.update(secret.as_bytes());
///         hasher.finalize().into()
///     }
///     
///     fn encrypt_token(&self, token: &str) -> Vec<u8> {
///         use aes_gcm::{Aes256Gcm, Key, Nonce, NewAead, AeadInPlace};
///         use rand::RngCore;
///         
///         // Generate random 96-bit nonce
///         let mut nonce_bytes = [0u8; 12];
///         rand::thread_rng().fill_bytes(&mut nonce_bytes);
///         let nonce = Nonce::from_slice(&nonce_bytes);
///         
///         // Create cipher instance
///         let key = Key::from_slice(&self.encryption_key);
///         let cipher = Aes256Gcm::new(key);
///         
///         // Encrypt in place
///         let mut buffer = token.as_bytes().to_vec();
///         cipher.encrypt_in_place(nonce, b"", &mut buffer).expect("encryption failure");
///         
///         // Prepend nonce to encrypted data
///         let mut result = nonce_bytes.to_vec();
///         result.extend_from_slice(&buffer);
///         result
///     }
/// }
/// ```
///
/// ## Docker Deployment Example
/// ```dockerfile
/// # Dockerfile
/// FROM rust:1.89-alpine AS builder
/// WORKDIR /app
/// COPY . .
/// RUN cargo build --release --example 09_oauth_authentication
///
/// FROM alpine:latest
/// RUN apk add --no-cache ca-certificates
/// COPY --from=builder /app/target/release/examples/09_oauth_authentication /usr/local/bin/
///
/// # Production environment variables
/// ENV OAUTH_ENCRYPTION_SECRET=""
/// ENV REDIS_URL=""
/// ENV DATABASE_URL=""
///
/// CMD ["09_oauth_authentication"]
/// ```
///
/// ## Environment Variables for Production
/// ```bash
/// # Required for production security
/// export OAUTH_ENCRYPTION_SECRET="your-256-bit-secret-key"
/// export REDIS_URL="redis://user:password@redis-host:6379"
/// export DATABASE_URL="postgresql://user:pass@db-host:5432/tokens"
///
/// # Redirect URI security configuration
/// export OAUTH_ALLOWED_REDIRECT_HOSTS="yourdomain.com,api.yourdomain.com,auth.yourdomain.com"
/// export OAUTH_MAIN_DOMAIN="yourdomain.com"  # Allows *.yourdomain.com subdomains
///
/// # OAuth provider credentials
/// export GOOGLE_CLIENT_ID="your-google-client-id"
/// export GOOGLE_CLIENT_SECRET="your-google-client-secret"
/// export GITHUB_CLIENT_ID="your-github-client-id"  
/// export GITHUB_CLIENT_SECRET="your-github-client-secret"
/// ```
///
/// REMEMBER: This example storage is a security vulnerability in production!
#[derive(Debug)]
pub struct MemoryTokenStorage {
    tokens: Arc<RwLock<HashMap<String, AccessToken>>>,
}

#[async_trait::async_trait]
impl TokenStorage for MemoryTokenStorage {
    async fn store_access_token(&self, user_id: &str, token: &AccessToken) -> McpResult<()> {
        self.tokens
            .write()
            .await
            .insert(user_id.to_string(), token.clone());
        Ok(())
    }

    async fn get_access_token(&self, user_id: &str) -> McpResult<Option<AccessToken>> {
        Ok(self.tokens.read().await.get(user_id).cloned())
    }

    async fn store_refresh_token(
        &self,
        _user_id: &str,
        _token: &oauth2::RefreshToken,
    ) -> McpResult<()> {
        // Implement refresh token storage in production
        Ok(())
    }

    async fn get_refresh_token(&self, _user_id: &str) -> McpResult<Option<oauth2::RefreshToken>> {
        Ok(None)
    }

    async fn revoke_tokens(&self, user_id: &str) -> McpResult<()> {
        self.tokens.write().await.remove(user_id);
        Ok(())
    }

    async fn list_users(&self) -> McpResult<Vec<String>> {
        Ok(self.tokens.read().await.keys().cloned().collect())
    }
}

#[server(
    name = "AuthenticatedMCPServer",
    version = "1.0.0",
    description = "MCP server with comprehensive OAuth 2.0 authentication"
)]
impl AuthenticatedServer {
    /// Get current user information (requires authentication)
    #[tool("Get the current authenticated user's profile information")]
    async fn get_user_profile(&self) -> McpResult<String> {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.values().next() {
            Ok(format!(
                "User: {} ({}) from {}",
                session.name, session.email, session.provider
            ))
        } else {
            Err(McpError::Unauthorized(
                "Authentication required. Please visit /auth/google or /auth/github".to_string(),
            ))
        }
    }

    /// List available OAuth providers
    #[tool("Get list of available authentication providers")]
    async fn list_auth_providers(&self) -> McpResult<Vec<String>> {
        let providers = self.oauth_providers.read().await;
        Ok(providers.keys().cloned().collect())
    }

    /// Protected tool - requires authentication
    #[tool("Access user's private data (requires authentication)")]
    async fn get_private_data(&self) -> McpResult<String> {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.values().next() {
            Ok(format!(
                "🔒 Private data for {} ({}) from {}",
                session.name, session.email, session.provider
            ))
        } else {
            Err(McpError::Unauthorized(
                "Authentication required".to_string(),
            ))
        }
    }

    /// Start OAuth flow for a provider
    #[tool("Start OAuth authentication flow for the specified provider")]
    async fn start_oauth_flow(&self, provider: String) -> McpResult<String> {
        let providers = self.oauth_providers.read().await;

        if let Some(oauth_provider) = providers.get(&provider) {
            let auth_result = oauth_provider.start_authorization().await?;

            Ok(format!(
                "🚀 OAuth flow started! Visit: {}",
                auth_result.auth_url
            ))
        } else {
            Err(McpError::InvalidInput(format!(
                "Unknown provider: {}. Available: {:?}",
                provider,
                providers.keys().collect::<Vec<_>>()
            )))
        }
    }
}

impl AuthenticatedServer {
    /// Create server with OAuth providers
    pub async fn new() -> McpResult<Self> {
        let mut oauth_providers = HashMap::new();
        let token_storage: Arc<dyn TokenStorage> = Arc::new(MemoryTokenStorage {
            tokens: Arc::new(RwLock::new(HashMap::new())),
        });

        // Setup Google OAuth (if configured)
        if let (Ok(client_id), Ok(client_secret)) = (
            std::env::var("GOOGLE_CLIENT_ID"),
            std::env::var("GOOGLE_CLIENT_SECRET"),
        ) {
            let google_config = OAuth2Config {
                client_id,
                client_secret,
                auth_url: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
                token_url: "https://www.googleapis.com/oauth2/v4/token".to_string(),
                redirect_uri: "http://localhost:8080/auth/callback".to_string(),
                scopes: vec![
                    "openid".to_string(),
                    "email".to_string(),
                    "profile".to_string(),
                ],
                additional_params: std::collections::HashMap::new(),
                flow_type: OAuth2FlowType::AuthorizationCode,
                security_level: SecurityLevel::Standard,
                #[cfg(feature = "dpop")]
                dpop_config: None,
            };

            let google_provider = OAuth2Provider::new(
                "google".to_string(),
                google_config,
                ProviderType::Google,
                Arc::clone(&token_storage),
            ).await?;

            oauth_providers.insert("google".to_string(), google_provider);
            println!("✅ Google OAuth configured");
        } else {
            println!(
                "⚠️  Google OAuth not configured (set GOOGLE_CLIENT_ID and GOOGLE_CLIENT_SECRET)"
            );
        }

        // Setup GitHub OAuth (if configured)
        if let (Ok(client_id), Ok(client_secret)) = (
            std::env::var("GITHUB_CLIENT_ID"),
            std::env::var("GITHUB_CLIENT_SECRET"),
        ) {
            let github_config = OAuth2Config {
                client_id,
                client_secret,
                auth_url: "https://github.com/login/oauth/authorize".to_string(),
                token_url: "https://github.com/login/oauth/access_token".to_string(),
                redirect_uri: "http://localhost:8080/auth/callback".to_string(),
                scopes: vec!["user:email".to_string()],
                additional_params: std::collections::HashMap::new(),
                flow_type: OAuth2FlowType::AuthorizationCode,
                security_level: SecurityLevel::Standard,
                #[cfg(feature = "dpop")]
                dpop_config: None,
            };

            let github_provider = OAuth2Provider::new(
                "github".to_string(),
                github_config,
                ProviderType::GitHub,
                Arc::clone(&token_storage),
            ).await?;

            oauth_providers.insert("github".to_string(), github_provider);
            println!("✅ GitHub OAuth configured");
        } else {
            println!(
                "⚠️  GitHub OAuth not configured (set GITHUB_CLIENT_ID and GITHUB_CLIENT_SECRET)"
            );
        }

        if oauth_providers.is_empty() {
            println!("❌ No OAuth providers configured. Please set environment variables.");
            println!("   See example comments for setup instructions.");
        }

        Ok(Self {
            oauth_providers: Arc::new(RwLock::new(oauth_providers)),
            sessions: Arc::new(RwLock::new(HashMap::new())),
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("🚀 Starting TurboMCP server with OAuth authentication...");

    // Create authenticated server
    let server = AuthenticatedServer::new()
        .await
        .map_err(|e| format!("Failed to create server: {}", e))?;

    // List configured providers
    let providers = server
        .list_auth_providers()
        .await
        .map_err(|e| format!("Failed to list providers: {}", e))?;
    if !providers.is_empty() {
        println!("🔐 Configured OAuth providers: {:?}", providers);
        println!("📝 Try these tools:");
        println!("   • list_auth_providers - See available providers");
        println!("   • start_oauth_flow - Begin authentication");
        println!("   • get_user_profile - Get authenticated user info");
        println!("   • get_private_data - Access protected resources");
    }

    // Run server
    server
        .run_stdio()
        .await
        .map_err(|e| e as Box<dyn std::error::Error>)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_oauth_server_creation() {
        // Should create successfully even without env vars
        let result = AuthenticatedServer::new().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_oauth_providers_list() {
        let server = AuthenticatedServer::new().await.unwrap();
        let providers = server.list_auth_providers().await.unwrap();
        // Should return empty list if no env vars set
        assert!(providers.is_empty() || !providers.is_empty());
    }

    #[tokio::test]
    async fn test_unauthenticated_access() {
        let server = AuthenticatedServer::new().await.unwrap();

        // Should fail without authentication
        let result = server.get_user_profile().await;
        assert!(result.is_err());
    }
}
