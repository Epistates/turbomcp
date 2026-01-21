//! Authentication support for WASM MCP servers.
//!
//! This module provides JWT validation using the Web Crypto API,
//! enabling authentication in Cloudflare Workers and other WASM environments.
//!
//! # Features
//!
//! - JWT signature verification using Web Crypto API
//! - JWKS fetching with caching support
//! - Support for RS256 and ES256 algorithms
//! - Cloudflare Access integration helpers
//!
//! # Example
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

pub use jwks::{Jwk, JwkSet, JwksCache, fetch_jwks};
pub use jwt::{CloudflareAccessAuthenticator, CloudflareAccessExtractor, WasmJwtAuthenticator};

// Re-export core auth types for convenience
pub use turbomcp_core::auth::{
    AuthError, Authenticator, Credential, CredentialExtractor, HeaderExtractor, JwtAlgorithm,
    JwtConfig, Principal, StandardClaims,
};
