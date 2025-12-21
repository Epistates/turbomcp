//! JSON-RPC request and response types
//!
//! This module re-exports the canonical JSON-RPC types from `turbomcp-protocol`
//! for HTTP boundary parsing.
//!
//! **Note**: This module is deprecated. Use `crate::axum::types` instead.
//! This file exists for backward compatibility with code that imports from
//! `crate::axum::query::json_rpc`.

// Re-export canonical types with backward-compatible names
pub use turbomcp_protocol::jsonrpc::JsonRpcError;
pub use turbomcp_protocol::jsonrpc::http::HttpJsonRpcRequest as JsonRpcRequest;
pub use turbomcp_protocol::jsonrpc::http::HttpJsonRpcResponse as JsonRpcResponse;
