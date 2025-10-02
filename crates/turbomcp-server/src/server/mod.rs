//! MCP server core implementation
//!
//! This module contains the decomposed server implementation with focused
//! modules for different responsibilities:
//!
//! - `core`: Main server implementation and middleware stack (Clone for sharing)
//! - `transport`: Transport message handling for JSON-RPC
//! - `builder`: Server builder pattern for construction
//! - `shutdown`: Graceful shutdown coordination

// Core modules
pub mod builder;
pub mod core;
pub mod shutdown;
pub mod transport;

// Re-export main types for backwards compatibility
pub use builder::ServerBuilder;
pub use core::McpServer;
pub use shutdown::ShutdownHandle;
