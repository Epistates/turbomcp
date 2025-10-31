//! Proxy module for bridging MCP servers across transports
//!
//! This module provides the core proxy functionality that enables universal
//! MCP transport adaptation. It allows ANY MCP-compliant server to be exposed
//! on ANY transport with turbomcp's world-class capabilities.
//!
//! ## Architecture
//!
//! ```text
//! Frontend (turbomcp-server)  ↔  Router  ↔  Backend (turbomcp-client)
//!   HTTP/WebSocket clients        Core       STDIO/HTTP/WebSocket server
//! ```
//!
//! ## Modules
//!
//! - `backend` - Backend connection management (turbomcp-client wrapper)
//! - `backends` - Concrete backend transport implementations (HTTP, etc.)
//! - `frontends` - Concrete frontend transport implementations (STDIO, etc.)
//! - `service` - Proxy service for Axum integration (Phase 2)
//! - `id_translator` - Bidirectional `MessageId` translation
//! - `metrics` - Performance and health metrics collection
//! - `auth` - Authentication and JWT signing for backend communication (optional)

#[cfg(feature = "auth")]
pub mod auth;
pub mod backend;
pub mod backends;
pub mod frontends;
pub mod id_translator;
pub mod metrics;
pub mod service;

#[cfg(feature = "auth")]
pub use auth::{JwtSigner, ProxyAuthConfig};
pub use backend::{BackendConfig, BackendConnector, BackendTransport};
pub use backends::HttpBackend;
pub use frontends::StdioFrontend;
pub use id_translator::IdTranslator;
pub use metrics::{AtomicMetrics, ProxyMetrics};
pub use service::ProxyService;
