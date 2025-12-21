//! JSON-RPC protocol types for HTTP transport
//!
//! This module re-exports the canonical JSON-RPC types from `turbomcp-protocol`
//! for HTTP boundary parsing. These lenient types accept any JSON-RPC message
//! structure and allow validation to happen at the handler level.
//!
//! # Design
//!
//! The canonical types in `turbomcp_protocol::jsonrpc::http` use lenient parsing:
//! - `jsonrpc` accepts any string (validated at handler level)
//! - `id` accepts any JSON value (string, number, or null)
//! - `params` is optional
//!
//! This enables proper JSON-RPC error responses when clients send non-compliant
//! requests, rather than failing at deserialization.

// Re-export canonical types with backward-compatible names
pub use turbomcp_protocol::jsonrpc::JsonRpcError;
pub use turbomcp_protocol::jsonrpc::http::HttpJsonRpcRequest as JsonRpcRequest;
pub use turbomcp_protocol::jsonrpc::http::HttpJsonRpcResponse as JsonRpcResponse;
