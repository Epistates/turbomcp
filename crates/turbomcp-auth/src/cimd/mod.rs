//! # Client ID Metadata Documents (CIMD)
//!
//! Implementation of OAuth 2.0 Client ID Metadata Documents as specified in:
//! - [draft-ietf-oauth-client-id-metadata-document-00](https://datatracker.ietf.org/doc/html/draft-ietf-oauth-client-id-metadata-document-00)
//! - MCP 2025-11-25 Specification (SEP-991)
//!
//! ## Overview
//!
//! Client ID Metadata Documents (CIMD) allow OAuth clients to use HTTPS URLs as their
//! `client_id` and host metadata at that URL, eliminating the need for traditional
//! Dynamic Client Registration (DCR).
//!
//! ## Key Features
//!
//! - **SSRF Protection**: Comprehensive protection against Server-Side Request Forgery
//! - **HTTP Caching**: Respects Cache-Control headers, never caches errors
//! - **Rate Limiting**: Per-client_id rate limiting to prevent abuse
//! - **Response Size Limits**: Default 5KB max (MCP spec recommendation)
//! - **Request Timeouts**: Configurable timeouts for network requests
//! - **Validation**: Complete metadata validation including URL verification
//!
//! ## Usage
//!
//! ```rust,ignore
//! use turbomcp_auth::cimd::{MetadataFetcher, ClientMetadata};
//! use turbomcp_auth::ssrf::SsrfValidator;
//!
//! // Create fetcher with SSRF protection
//! let ssrf_validator = SsrfValidator::default();
//! let fetcher = MetadataFetcher::new(ssrf_validator)?;
//!
//! // Fetch and validate metadata
//! let client_id = "https://app.example.com/oauth/client-metadata.json";
//! let validated = fetcher.fetch(client_id).await?;
//!
//! // Use the validated metadata
//! assert!(validated.is_redirect_uri_allowed("http://localhost:3000/callback"));
//! ```
//!
//! ## Security Considerations
//!
//! Authorization servers implementing CIMD **MUST**:
//!
//! 1. **Validate URLs before fetching** - Use SSRF protection to block:
//!    - Private networks (10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16)
//!    - Localhost (127.0.0.0/8, ::1)
//!    - Link-local addresses (169.254.0.0/16, fe80::/10)
//!    - Cloud metadata endpoints (169.254.169.254)
//!
//! 2. **Implement response limits** - Default 5KB max per MCP spec
//!
//! 3. **Implement timeouts** - Aggressive timeouts on network requests
//!
//! 4. **Cache aggressively** - Minimize repeated fetches per client
//!
//! 5. **Never cache errors** - Only cache successful responses
//!
//! 6. **Implement rate limiting** - Per-client fetch limits
//!
//! 7. **Monitor patterns** - Alert on unusual metadata fetch behavior
//!
//! 8. **Authenticate first** - Only fetch metadata after user authentication
//!
//! ## MCP Specification Compliance
//!
//! This implementation follows the MCP 2025-11-25 specification requirements:
//!
//! - ✅ HTTPS URLs as client_id
//! - ✅ On-demand metadata fetching with caching
//! - ✅ Domain-based trust via HTTPS certificates
//! - ✅ SSRF protection with comprehensive IP validation
//! - ✅ Response size limits (5KB default)
//! - ✅ Request timeouts (5s default)
//! - ✅ HTTP caching with Cache-Control respect
//! - ✅ No error caching
//! - ✅ Rate limiting per-client
//! - ✅ Validate client_id in document matches URL
//!
//! ## Example Metadata Document
//!
//! ```json
//! {
//!   "client_id": "https://app.example.com/oauth/client-metadata.json",
//!   "client_name": "Example MCP Client",
//!   "client_uri": "https://app.example.com",
//!   "logo_uri": "https://app.example.com/logo.png",
//!   "redirect_uris": [
//!     "http://127.0.0.1:3000/callback",
//!     "http://localhost:3000/callback"
//!   ],
//!   "grant_types": ["authorization_code"],
//!   "response_types": ["code"],
//!   "token_endpoint_auth_method": "none"
//! }
//! ```

pub mod fetcher;
pub mod types;

// Re-export main types
pub use fetcher::{CacheStats, FetcherConfig, FetcherError, MetadataFetcher};
pub use types::{ClientMetadata, ClientMetadataError, ValidatedClientMetadata};
