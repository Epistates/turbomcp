//! Query parameter types for different transport endpoints
//!
//! This module contains query parameter structures for SSE, WebSocket,
//! and JSON-RPC communication endpoints.

#[cfg(feature = "http")]
pub mod json_rpc;
#[cfg(feature = "http")]
pub mod sse;
#[cfg(feature = "http")]
pub mod websocket;

// Re-export all query types
#[cfg(feature = "http")]
pub use json_rpc::*;
#[cfg(feature = "http")]
pub use sse::*;
#[cfg(feature = "http")]
pub use websocket::*;
