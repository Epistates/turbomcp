//! HTTP handlers for MCP endpoints
//!
//! This module contains all the HTTP endpoint handlers for the TurboMCP Axum integration.
//! Each handler is focused on a specific responsibility and follows Axum best practices.
//!
//! ## Handlers
//!
//! - [`root`] - Basic server information endpoint
//! - [`json_rpc`] - Core JSON-RPC request processing
//! - [`capabilities`] - MCP server capabilities endpoint
//! - [`sse`] - Server-Sent Events for real-time communication
//! - [`websocket`] - WebSocket upgrade and bidirectional communication
//! - [`health`] - Health check endpoint for monitoring
//! - [`metrics`] - Detailed metrics endpoint for observability

pub mod root;
pub mod json_rpc;
pub mod capabilities;
pub mod sse;
pub mod websocket;
pub mod health;
pub mod metrics;

// Re-export all handler functions for convenience
pub use root::root_handler;
pub use json_rpc::json_rpc_handler;
pub use capabilities::capabilities_handler;
pub use sse::sse_handler;
pub use websocket::websocket_handler;
pub use health::health_handler;
pub use metrics::metrics_handler;

// Re-export SessionInfo from tower (canonical implementation)
pub use crate::tower::SessionInfo;