//! OAuth 2.1 Implementation
//!
//! This module provides an OAuth 2.1 implementation with:
//! - Authorization Code flow with PKCE (RFC 7636)
//! - Refresh tokens
//! - Resource Indicators (RFC 8707) - **MCP Required**
//! - Protected Resource Metadata (RFC 9728) - **MCP Required**
//! - Dynamic Client Registration (RFC 7591)
//! - DPoP integration (RFC 9449)
//!
//! ## Submodules
//!
//! - `client` - OAuth2Client for basic operations
//! - `resource` - RFC 8707 Resource Indicators (MCP required)
//! - `validation` - URI and security validation
//!
//! ## MCP Compliance
//!
//! This implementation follows MCP specification requirements:
//! - RFC 8707 resource parameters MUST be included in all OAuth flows
//! - Tokens MUST be bound to specific MCP servers via audience claims
//! - PKCE MUST be used for authorization code flows

pub mod client;
pub mod dcr;
pub mod http_client;
pub mod resource;
pub mod validation;

// Re-export client types
pub use client::OAuth2Client;

// Re-export HTTP client adapter
pub use http_client::OAuth2HttpClient;

// Re-export DCR types (RFC 7591)
pub use dcr::{DcrBuilder, DcrClient, RegistrationRequest, RegistrationResponse};

// Re-export resource validation (RFC 8707)
pub use resource::validate_resource_uri;

// Re-export validation functions
pub use validation::*;
