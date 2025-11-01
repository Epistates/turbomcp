//! turbomcp-proxy: Universal MCP Adapter/Generator
//!
//! A universal tool that works with ANY MCP server implementation (`TurboMCP`, Python SDK,
//! TypeScript SDK, custom implementations). It discovers server capabilities via the MCP
//! protocol and dynamically generates adapters for different transports and protocols.
//!
//! # Features
//!
//! - **Universal Compatibility**: Works with any MCP 2025-06-18 compliant server
//! - **Zero Configuration**: Discovers capabilities via introspection
//! - **Multiple Modes**: Runtime (fast), Codegen (optimized), Schema export
//! - **Protocol Translation**: Expose MCP as REST API, GraphQL, gRPC
//!
//! # Quick Start
//!
//! ```bash
//! # Inspect any MCP server
//! turbomcp-proxy inspect stdio --cmd "python my-server.py"
//!
//! # Expose STDIO server over HTTP/SSE
//! turbomcp-proxy serve \
//!   --backend stdio --cmd "python my-server.py" \
//!   --frontend http --bind 0.0.0.0:3000
//! ```
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │ Introspection Layer                                     │
//! │ • McpIntrospector: Discovers server capabilities       │
//! │ • ServerSpec: Complete server description               │
//! └─────────────────────────────────────────────────────────┘
//!                           ↓
//! ┌─────────────────────────────────────────────────────────┐
//! │ Generation Layer                                        │
//! │ • RuntimeProxyBuilder: Dynamic, no codegen              │
//! │ • RustCodeGenerator: Optimized Rust source              │
//! └─────────────────────────────────────────────────────────┘
//!                           ↓
//! ┌─────────────────────────────────────────────────────────┐
//! │ Adapter Layer                                           │
//! │ • Transport Adapters: STDIO ↔ HTTP/SSE ↔ WebSocket     │
//! │ • Protocol Adapters: MCP → REST API / GraphQL          │
//! └─────────────────────────────────────────────────────────┘
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

// Public modules
#[cfg(feature = "introspection")]
pub mod introspection;

#[cfg(feature = "runtime")]
pub mod runtime;

#[cfg(feature = "codegen")]
pub mod codegen;

#[cfg(feature = "schema")]
pub mod schema;

pub mod adapters;
pub mod config;
pub mod error;
pub mod proxy;

#[cfg(feature = "cli")]
pub mod cli;

// Re-exports for convenience
pub use error::{ProxyError, ProxyResult};

/// Prelude module for common imports
pub mod prelude {
    pub use crate::config::{BackendConfig, FrontendType};
    pub use crate::error::{ProxyError, ProxyResult};

    #[cfg(feature = "introspection")]
    pub use crate::introspection::{
        McpBackend, McpIntrospector, PromptSpec, ResourceSpec, ServerSpec, StdioBackend, ToolSpec,
    };

    #[cfg(feature = "runtime")]
    pub use crate::runtime::{RuntimeProxy, RuntimeProxyBuilder};

    #[cfg(feature = "codegen")]
    pub use crate::codegen::RustCodeGenerator;

    #[cfg(feature = "rest")]
    pub use crate::adapters::rest::{RestAdapter, RestAdapterConfig};

    #[cfg(feature = "graphql")]
    pub use crate::adapters::graphql::{GraphQLAdapter, GraphQLAdapterConfig};

    // Proxy components
    pub use crate::proxy::{
        AtomicMetrics, BackendConnector, BackendTransport, IdTranslator, ProxyMetrics, ProxyService,
    };

    // Frontend transports
    pub use crate::proxy::frontends::{TcpFrontend, TcpFrontendConfig, UnixFrontend, UnixFrontendConfig};
}

/// Version of turbomcp-proxy
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// MCP protocol version supported
pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";
