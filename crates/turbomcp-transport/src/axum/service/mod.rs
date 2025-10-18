//! Service layer for MCP implementation
//!
//! This module provides the service trait definition and application state
//! management for MCP services in Axum applications.

pub mod interface;
pub mod state;

// Re-export main service types
pub use interface::*;
pub use state::*;
