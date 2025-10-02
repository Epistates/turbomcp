//! TurboMCP Authentication Module
//!
//! This module provides comprehensive OAuth 2.1, API key authentication,
//! and authorization functionality for the TurboMCP protocol.
//!
//! ## Architecture
//!
//! The auth module is organized into the following submodules:
//! - `config` - Configuration types for authentication providers
//! - `types` - Core authentication types (AuthContext, UserInfo, etc.)
//! - `api_key` - API key authentication provider
//! - `manager` - Authentication manager for provider orchestration
//! - `middleware` - Authentication middleware
//! - `oauth2` - OAuth 2.1 implementation with RFC compliance
//! - `dpop` - DPoP (RFC 9449) proof-of-possession tokens (feature-gated)

// Submodules
pub mod config;
pub mod manager;
pub mod oauth2;
pub mod providers;
pub mod types;

#[cfg(feature = "dpop")]
pub mod dpop;

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

// Re-export OAuth2Provider from auth_impl (temporary)
#[doc(inline)]
pub use crate::auth_impl::OAuth2Provider;
