//! Axum Integration Layer for TurboMCP
//!
//! This module provides seamless integration with Axum routers, enabling the
//! "bring your own server" philosophy while providing opinionated defaults for
//! rapid development.
//!
//! NOTE: This entire module is only compiled when feature="http" is enabled.
//! See lib.rs for the module-level feature gate.

pub mod config;
pub mod handlers;
pub mod query;
pub mod router;
pub mod service;
pub mod types;

#[cfg(test)]
pub mod tests;

// Re-export main public types (avoiding glob conflicts)
pub use config::{
    AuthConfig, CorsConfig, Environment, McpServerConfig, RateLimitConfig, SecurityConfig,
    TlsConfig,
};
pub use handlers::{
    SessionInfo, capabilities_handler, health_handler, json_rpc_handler, metrics_handler,
    sse_handler, websocket_handler,
};
pub use router::AxumMcpExt;
pub use service::{McpAppState, McpService};
pub use types::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, SseQuery, WebSocketQuery};
