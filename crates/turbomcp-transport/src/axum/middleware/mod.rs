//! Middleware components for MCP HTTP endpoints
//!
//! This module contains all the middleware components used in the TurboMCP Axum
//! integration. Each middleware is focused on a specific cross-cutting concern
//! and can be composed together to build a comprehensive middleware stack.
//!
//! ## Middleware Components
//!
//! - [`mcp`] - Basic MCP session management
//! - [`security`] - Security headers application
//! - [`rate_limit`] - Request rate limiting
//! - [`auth`] - Authentication and authorization

pub mod mcp;
pub mod security;
pub mod rate_limit;
pub mod auth;

// Re-export all middleware functions for convenience
pub use mcp::mcp_middleware;
pub use security::security_headers_middleware;
pub use rate_limit::rate_limiting_middleware;
pub use auth::authentication_middleware;