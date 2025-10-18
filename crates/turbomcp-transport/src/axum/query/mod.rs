//! Query parameter types for different transport endpoints
//!
//! This module contains query parameter structures for SSE, WebSocket,
//! and JSON-RPC communication endpoints.

pub mod json_rpc;
pub mod sse;
pub mod websocket;

// Re-export all query types
pub use json_rpc::*;
pub use sse::*;
pub use websocket::*;
