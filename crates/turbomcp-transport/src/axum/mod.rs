//! Axum Integration Layer for TurboMCP
//!
//! This module provides seamless integration with Axum routers, enabling the
//! "bring your own server" philosophy while providing opinionated defaults for
//! rapid development.

#[cfg(feature = "http")]
pub mod config;
#[cfg(feature = "http")]
pub mod handlers;
#[cfg(feature = "http")]
pub mod query;
#[cfg(feature = "http")]
pub mod router;
#[cfg(feature = "http")]
pub mod service;
#[cfg(feature = "http")]
pub mod types;

#[cfg(test)]
#[cfg(feature = "http")]
pub mod tests;

// Re-export main public types (avoiding glob conflicts)
#[cfg(feature = "http")]
pub use config::{McpServerConfig, AuthConfig, CorsConfig, SecurityConfig, RateLimitConfig, TlsConfig, Environment};
#[cfg(feature = "http")]
pub use handlers::{
    json_rpc_handler, capabilities_handler, sse_handler,
    websocket_handler, health_handler, metrics_handler, SessionInfo
};
#[cfg(feature = "http")]
pub use router::AxumMcpExt;
#[cfg(feature = "http")]
pub use service::{McpService, McpAppState};
#[cfg(feature = "http")]
pub use types::{JsonRpcRequest, JsonRpcResponse, JsonRpcError, SseQuery, WebSocketQuery};
