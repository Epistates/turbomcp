//! OAuth 2.1 Implementation
//!
//! This module provides an OAuth 2.1 implementation with:
//! - Authorization Code flow with PKCE (RFC 7636)
//! - Refresh tokens
//! - Resource Indicators (RFC 8707)
//! - Protected Resource Metadata (RFC 9728)
//! - Dynamic Client Registration (RFC 7591)
//! - DPoP integration (RFC 9449)
//!
//! ## Submodules
//!
//! - `client` - OAuth2Client for basic operations
//! - `authorization` - Authorization flow logic
//! - `token` - Token management and refresh
//! - `validation` - URI and security validation
//! - `rfc_compliance` - RFC-specific implementations

pub mod client;
pub mod validation;

// Re-export client types
pub use client::OAuth2Client;

// Re-export validation functions
pub use validation::*;
