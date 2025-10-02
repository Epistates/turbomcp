//! Service layer for MCP implementation
//!
//! This module provides the service trait definition and application state
//! management for MCP services in Axum applications.

#[cfg(feature = "http")]
pub mod interface;
#[cfg(feature = "http")]
pub mod state;

// Re-export main service types
#[cfg(feature = "http")]
pub use interface::*;
#[cfg(feature = "http")]
pub use state::*;
