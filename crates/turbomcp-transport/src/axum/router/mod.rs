//! Router extension functionality for MCP integration
//!
//! This module provides the AxumMcpExt trait and related functionality
//! for seamlessly integrating MCP capabilities with Axum routers.

pub mod builder;
pub mod extension;

// Re-export main router types and functions (pub(crate) since not used externally)
pub use extension::*;
