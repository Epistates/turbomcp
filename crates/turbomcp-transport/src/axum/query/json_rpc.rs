//! JSON-RPC request and response types for HTTP boundary parsing.
//!
//! Re-exports the canonical JSON-RPC types from `turbomcp-protocol`.

pub use turbomcp_protocol::jsonrpc::JsonRpcError;
pub use turbomcp_protocol::jsonrpc::http::HttpJsonRpcRequest as JsonRpcRequest;
pub use turbomcp_protocol::jsonrpc::http::HttpJsonRpcResponse as JsonRpcResponse;
