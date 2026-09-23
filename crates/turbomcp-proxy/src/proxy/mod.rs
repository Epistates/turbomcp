//! Proxy module for bridging MCP servers across transports
//!
//! This module provides the core proxy functionality that enables universal
//! MCP transport adaptation. It allows ANY MCP-compliant server to be exposed
//! on ANY transport with turbomcp's world-class capabilities.
//!
//! ## Architecture
//!
//! ```text
//! Frontend (turbomcp-server)  ↔  ProxyService  ↔  Backend (turbomcp-client)
//!   STDIO/HTTP/WebSocket clients    McpHandler      STDIO/HTTP/TCP/Unix/WebSocket server
//! ```
//!
//! ## Modules
//!
//! - `backend` - Backend connection management (turbomcp-client wrapper)
//! - `frontends` - Concrete frontend transport implementations (STDIO, etc.)
//! - `service` - `ProxyService`, the `McpHandler` every frontend serves
//! - `id_translator` - Bidirectional `MessageId` translation
//! - `metrics` - Performance and health metrics collection
//! - `auth` - Authentication and JWT signing for backend communication (optional)

#[cfg(feature = "auth")]
pub mod auth;
#[cfg(feature = "introspection")]
pub mod backend;
#[cfg(feature = "introspection")]
pub mod frontends;
pub mod id_translator;
pub mod metrics;
#[cfg(feature = "runtime")]
pub mod service;

#[cfg(feature = "auth")]
pub use auth::{JwtSigner, ProxyAuthConfig};
#[cfg(feature = "introspection")]
pub use backend::{BackendConfig, BackendConnector, BackendTransport};
#[cfg(feature = "runtime")]
pub use frontends::StdioFrontend;
pub use id_translator::IdTranslator;
pub use metrics::{AtomicMetrics, ProxyMetrics};
#[cfg(feature = "runtime")]
pub use service::ProxyService;
