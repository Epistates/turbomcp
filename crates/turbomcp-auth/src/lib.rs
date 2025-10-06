//! # TurboMCP Auth - OAuth 2.1 and Authentication
//!
//! Comprehensive OAuth 2.1, API key authentication, and authorization functionality
//! for the TurboMCP protocol with MCP specification compliance.
//!
//! ## Features
//!
//! - **OAuth 2.1** - RFC 8707/9728/7591 compliant with MCP resource binding
//! - **Multi-Provider** - Google, GitHub, Microsoft with PKCE and security hardening
//! - **API Key Auth** - Simple API key authentication provider
//! - **Session Management** - Secure session handling and token management
//! - **DPoP Support** - Optional RFC 9449 proof-of-possession (feature: `dpop`)
//!
//! ## Architecture
//!
//! - `config` - Configuration types for authentication providers
//! - `types` - Core authentication types (AuthContext, UserInfo, etc.)
//! - `providers` - Authentication provider implementations
//!   - `api_key` - API key authentication
//! - `manager` - Authentication manager for provider orchestration
//! - `oauth2` - OAuth 2.1 implementation with RFC compliance
//!
//! ## Feature Flags
//!
//! - `default` - Core authentication (no optional features)
//! - `dpop` - Enable DPoP (RFC 9449) support via `turbomcp-dpop`

// Submodules
pub mod config;
pub mod manager;
pub mod oauth2;
pub mod providers;
pub mod types;

// Re-export configuration types
#[doc(inline)]
pub use config::*;

// Re-export core types
#[doc(inline)]
pub use types::*;

// Re-export providers
#[doc(inline)]
pub use providers::*;

// Re-export manager
#[doc(inline)]
pub use manager::AuthManager;

// Re-export DPoP types when feature is enabled
#[cfg(feature = "dpop")]
pub use turbomcp_dpop as dpop;
