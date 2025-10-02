//! Configuration management for Axum MCP integration
//!
//! This module provides comprehensive configuration types for all aspects
//! of the MCP server including CORS, security, rate limiting, TLS, and authentication.

#[cfg(feature = "http")]
pub mod auth;
#[cfg(feature = "http")]
pub mod cors;
#[cfg(feature = "http")]
pub mod environment;
#[cfg(feature = "http")]
pub mod rate_limit;
#[cfg(feature = "http")]
pub mod security;
#[cfg(feature = "http")]
pub mod server;
#[cfg(feature = "http")]
pub mod tls;

// Re-export all configuration types
#[cfg(feature = "http")]
pub use auth::*;
#[cfg(feature = "http")]
pub use cors::*;
#[cfg(feature = "http")]
pub use environment::*;
#[cfg(feature = "http")]
pub use rate_limit::*;
#[cfg(feature = "http")]
pub use security::*;
#[cfg(feature = "http")]
pub use server::*;
#[cfg(feature = "http")]
pub use tls::*;
