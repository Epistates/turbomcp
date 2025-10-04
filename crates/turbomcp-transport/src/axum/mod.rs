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
pub use config::{
    AuthConfig, CorsConfig, Environment, McpServerConfig, RateLimitConfig, SecurityConfig,
    TlsConfig,
};
#[cfg(feature = "http")]
pub use handlers::{
    SessionInfo, capabilities_handler, health_handler, json_rpc_handler, metrics_handler,
    sse_handler, websocket_handler,
};
#[cfg(feature = "http")]
pub use router::AxumMcpExt;
#[cfg(feature = "http")]
pub use service::{McpAppState, McpService};
#[cfg(feature = "http")]
pub use types::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, SseQuery, WebSocketQuery};
