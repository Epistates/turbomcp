//! Authentication support for WASM MCP servers.
//!
//! This module provides comprehensive authentication for WASM environments:
//!
//! - **JWT Validation** - Validate incoming JWTs using Web Crypto API
//! - **OAuth 2.1 Provider** - Full authorization server for issuing tokens
//!
//! # JWT Validation
//!
//! ```ignore
//! use turbomcp_wasm::auth::{WasmJwtAuthenticator, JwtConfig};
//! use turbomcp_core::auth::{Authenticator, Credential};
//!
//! // Configure JWT validation
//! let config = JwtConfig::new()
//!     .issuer("https://auth.example.com")
//!     .audience("my-mcp-server");
//!
//! // Create authenticator with JWKS endpoint
//! let auth = WasmJwtAuthenticator::with_jwks(
//!     "https://auth.example.com/.well-known/jwks.json",
//!     config,
//! );
//!
//! // Validate a JWT
//! let credential = Credential::bearer("eyJ...");
//! let principal = auth.authenticate(&credential).await?;
//! println!("Authenticated: {}", principal.subject);
//! ```
//!
//! # OAuth 2.1 Provider
//!
//! ```ignore
//! use turbomcp_wasm::auth::provider::{OAuthProvider, OAuthProviderConfig, ClientConfig};
//!
//! let config = OAuthProviderConfig::new("https://my-mcp-server.workers.dev")
//!     .with_client(ClientConfig::public(
//!         "my-client-id",
//!         vec!["https://app.example.com/callback"],
//!     ))
//!     .with_scopes(vec!["read".to_string(), "write".to_string()]);
//!
//! let oauth = OAuthProvider::new(config);
//!
//! // In your worker:
//! #[event(fetch)]
//! async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
//!     let url = req.url()?;
//!     let path = url.path();
//!
//!     // Handle OAuth endpoints
//!     if path.starts_with("/oauth/") || path.starts_with("/.well-known/") {
//!         return oauth.handle(req).await;
//!     }
//!
//!     // Handle MCP endpoints
//!     let server = MyMcpServer::new();
//!     server.handle(req).await
//! }
//! ```
//!
//! # Cloudflare Access Integration
//!
//! For Cloudflare Access, use the helper that validates CF-Access-JWT-Assertion:
//!
//! ```ignore
//! use turbomcp_wasm::auth::CloudflareAccessAuthenticator;
//!
//! // Configure for your Cloudflare Access application
//! let auth = CloudflareAccessAuthenticator::new(
//!     "your-team.cloudflareaccess.com",
//!     "your-audience-tag",
//! );
//!
//! // Extract principal from request
//! let principal = auth.authenticate_request(&request).await?;
//! ```

mod jwks;
mod jwt;
pub mod provider;

pub use jwks::{Jwk, JwkSet, JwksCache, fetch_jwks};
pub use jwt::{CloudflareAccessAuthenticator, CloudflareAccessExtractor, WasmJwtAuthenticator};

// Re-export core auth types for convenience
pub use turbomcp_core::auth::{
    AuthError, Authenticator, Credential, CredentialExtractor, HeaderExtractor, JwtAlgorithm,
    JwtConfig, Principal, StandardClaims,
};

// Re-export commonly used provider types at the auth module level
pub use provider::{ClientConfig, OAuthError, OAuthProvider, OAuthProviderConfig, TokenResponse};
