//! # Authorization Server Discovery
//!
//! Support for OAuth 2.0 Authorization Server Metadata (RFC 8414) and
//! OpenID Connect Discovery 1.0 as required by MCP 2025-11-25 specification.
//!
//! ## Overview
//!
//! This module provides secure discovery of OAuth 2.0 and OpenID Connect
//! provider metadata with built-in SSRF protection, caching, and multi-endpoint
//! support. It implements the MCP requirement that servers MUST provide
//! OAuth 2.0 Authorization Server Metadata per RFC 8414.
//!
//! ## Discovery Endpoint Priority
//!
//! The fetcher tries multiple discovery endpoints in order:
//!
//! 1. **RFC 8414** (OAuth 2.0): `/.well-known/oauth-authorization-server[/path]`
//! 2. **OIDC Discovery 1.0** (fallback): `/.well-known/openid-configuration`
//!
//! ## Security Features
//!
//! - **SSRF Protection**: All URLs validated before requests (blocks private networks, localhost, cloud metadata)
//! - **Size Limits**: Response size capped at 10KB (configurable)
//! - **Timeouts**: 5-second request timeout (configurable)
//! - **No Redirects**: Redirect following disabled for security
//! - **HTTPS Only**: Issuer URLs must use HTTPS scheme
//!
//! ## Caching Strategy
//!
//! - Respects HTTP `Cache-Control` headers (`max-age`, `no-cache`, `no-store`)
//! - Default cache TTL: 1 hour (if no headers present)
//! - Maximum cache TTL: 24 hours (capped)
//! - Per-issuer cache with automatic expiration
//!
//! ## Usage Example
//!
//! ```rust
//! use turbomcp_auth::discovery::{DiscoveryFetcher, FetcherConfig};
//! use turbomcp_auth::ssrf::SsrfValidator;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create SSRF validator
//! let ssrf_validator = SsrfValidator::default();
//!
//! // Create discovery fetcher
//! let fetcher = DiscoveryFetcher::new(ssrf_validator)?;
//!
//! // Fetch discovery metadata
//! let metadata = fetcher.fetch("https://accounts.google.com").await?;
//!
//! // Access OAuth2 endpoints
//! let auth_endpoint = &metadata.oauth2().authorization_endpoint;
//! let token_endpoint = &metadata.oauth2().token_endpoint;
//!
//! // Check PKCE support
//! if metadata.oauth2().supports_pkce() {
//!     println!("Provider supports PKCE");
//! }
//!
//! // Access OIDC-specific endpoints if available
//! if let Some(oidc) = metadata.oidc() {
//!     let userinfo_endpoint = &oidc.userinfo_endpoint;
//!     println!("UserInfo endpoint: {}", userinfo_endpoint);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Standards Compliance
//!
//! - **RFC 8414**: OAuth 2.0 Authorization Server Metadata
//! - **OpenID Connect Discovery 1.0**: OIDC provider configuration
//! - **MCP 2025-11-25**: Multi-endpoint discovery requirement
//!
//! ## Related Modules
//!
//! - [`crate::ssrf`]: SSRF protection (required dependency)
//! - [`crate::cimd`]: Client ID Metadata Documents (complementary feature)
//! - [`crate::oauth2`]: OAuth 2.1 client (uses discovery metadata)

mod fetcher;
mod types;

pub use fetcher::{CacheStats, DiscoveryFetcher, FetcherConfig, FetcherError};
pub use types::{
    AuthorizationServerMetadata, DiscoveryError, DiscoveryMetadata, OIDCProviderMetadata,
    ValidatedDiscoveryMetadata,
};
