//! Protocol types for Axum integration
//!
//! This module provides all the protocol types needed for MCP over HTTP
//! including JSON-RPC structures and query parameters.

pub mod json_rpc;
pub mod query;

// Re-export all types for backward compatibility
pub use json_rpc::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};
pub use query::{SseQuery, WebSocketQuery};