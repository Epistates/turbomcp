//! # TurboMCP Transport
//!
//! Transport layer implementations for the Model Context Protocol with runtime
//! selection, comprehensive fault tolerance, and multiple protocol support.
//!
//! ## Supported Transports
//!
//! - **STDIO**: Standard input/output for command-line MCP servers (always available)
//! - **TCP**: Direct TCP socket communication for network deployments
//! - **Unix Sockets**: Fast local inter-process communication
//! - **HTTP/SSE**: HTTP with Server-Sent Events (client-oriented)
//! - **WebSocket**: Full-duplex communication (client-oriented)
//!
//! ## Reliability Features
//!
//! - **Circuit Breakers**: Automatic fault detection and recovery mechanisms
//! - **Retry Logic**: Configurable exponential backoff with jitter
//! - **Health Monitoring**: Real-time transport health status tracking
//! - **Connection Pooling**: Efficient connection reuse and management
//! - **Message Deduplication**: Prevention of duplicate message processing
//! - **Graceful Degradation**: Maintained service availability during failures
//!
//! ## Module Organization
//!
//! ```text
//! turbomcp-transport/
//! ├── core/           # Core transport traits and error types
//! ├── robustness/     # Circuit breakers, retry logic, health checks
//! ├── stdio/          # Standard I/O transport implementation
//! ├── http/           # HTTP/SSE transport implementation
//! ├── websocket/      # WebSocket transport implementation
//! ├── tcp/            # TCP socket transport implementation
//! ├── unix/           # Unix domain socket implementation
//! ├── compression/    # Message compression support
//! ├── pool/           # Connection pooling utilities
//! └── metrics/        # Transport performance metrics
//! ```

#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub,
    clippy::all
)]
#![deny(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![allow(
    clippy::module_name_repetitions,
    clippy::missing_errors_doc,  // Error documentation in progress
    clippy::cast_possible_truncation,  // Intentional in metrics code
    clippy::must_use_candidate,  // Too pedantic for library APIs
    clippy::return_self_not_must_use,  // Constructor methods don't need must_use
    clippy::struct_excessive_bools,  // Sometimes bools are the right design
    clippy::missing_panics_doc,  // Panic docs added where genuinely needed
    clippy::default_trait_access  // Default::default() is sometimes clearer
)]

pub mod core;

#[cfg(feature = "stdio")]
pub mod stdio;

// Tower service integration
pub mod tower;

// Server features for handler management
#[cfg(feature = "server")]
pub mod server;

#[cfg(feature = "http")]
pub mod axum_integration;

#[cfg(feature = "websocket")]
pub mod websocket;

#[cfg(feature = "tcp")]
pub mod tcp;

#[cfg(feature = "unix")]
pub mod unix;

#[cfg(feature = "tls")]
pub mod tls;

pub mod child_process;

#[cfg(feature = "compression")]
pub mod compression;

pub mod config;
pub mod metrics;
pub mod pool;
pub mod robustness;

// DPoP integration (optional feature)
#[cfg(feature = "dpop")]
#[cfg_attr(docsrs, doc(cfg(feature = "dpop")))]
pub mod dpop;

// Re-export core transport traits and types
pub use core::{
    Transport, TransportCapabilities, TransportConfig, TransportError, TransportEvent,
    TransportMessage, TransportMetrics, TransportResult, TransportState, TransportType,
};

// Re-export transport implementations
#[cfg(feature = "stdio")]
pub use stdio::StdioTransport;

#[cfg(feature = "tls")]
pub use tls::{
    CertPinningConfig, CertValidationConfig, ClientAuthMode, TlsConfig, TlsError, TlsTransport,
    TlsVersion,
};

// Re-export Tower integration
pub use tower::{SessionInfo, SessionManager, TowerTransportAdapter};

// Re-export server features
#[cfg(feature = "server")]
pub use server::{
    AuthContext, HandlerRegistry, PromptHandler, RequestContext, ResourceHandler, ResourceInfo,
    ResourceMetadata, SecurityContext, SecurityLevel, TokenInfo, ToolCapabilities, ToolHandler,
    TraceContext,
};

// Re-export Axum integration
#[cfg(feature = "http")]
pub use axum_integration::{AxumMcpExt, McpAppState, McpServerConfig, McpService};

#[cfg(feature = "websocket")]
pub use websocket::WebSocketTransport;

#[cfg(feature = "tcp")]
pub use tcp::TcpTransport;

#[cfg(feature = "unix")]
pub use unix::UnixTransport;

// Re-export child process transport (always available)
pub use child_process::{ChildProcessConfig, ChildProcessTransport};

// Re-export utilities
pub use config::TransportConfigBuilder;
pub use pool::ConnectionPool;
pub use robustness::{
    CircuitBreakerConfig, CircuitBreakerStats, CircuitState, HealthCheckConfig, HealthInfo,
    HealthStatus, RetryConfig, RobustTransport,
};

// Re-export DPoP integration
#[cfg(feature = "dpop")]
pub use dpop::{DpopMessageBuilder, DpopMetadataExt, DpopTransportConfig, DpopTransportExt};

/// Transport feature detection
#[derive(Debug)]
pub struct Features;

impl Features {
    /// Check if stdio transport is available
    #[must_use]
    pub const fn has_stdio() -> bool {
        cfg!(feature = "stdio")
    }

    /// Check if HTTP transport is available
    #[must_use]
    pub const fn has_http() -> bool {
        cfg!(feature = "http")
    }

    /// Check if WebSocket transport is available
    #[must_use]
    pub const fn has_websocket() -> bool {
        cfg!(feature = "websocket")
    }

    /// Check if TCP transport is available
    #[must_use]
    pub const fn has_tcp() -> bool {
        cfg!(feature = "tcp")
    }

    /// Check if Unix socket transport is available
    #[must_use]
    pub const fn has_unix() -> bool {
        cfg!(feature = "unix")
    }

    /// Check if compression support is available
    #[must_use]
    pub const fn has_compression() -> bool {
        cfg!(feature = "compression")
    }

    /// Check if TLS support is available
    #[must_use]
    pub const fn has_tls() -> bool {
        cfg!(feature = "tls")
    }

    /// Check if child process transport is available (always true)
    #[must_use]
    pub const fn has_child_process() -> bool {
        true
    }

    /// Get list of available transport types
    #[must_use]
    pub fn available_transports() -> Vec<TransportType> {
        let mut transports = Vec::new();

        if Self::has_stdio() {
            transports.push(TransportType::Stdio);
        }
        if Self::has_http() {
            transports.push(TransportType::Http);
        }
        if Self::has_websocket() {
            transports.push(TransportType::WebSocket);
        }
        if Self::has_tcp() {
            transports.push(TransportType::Tcp);
        }
        if Self::has_unix() {
            transports.push(TransportType::Unix);
        }
        if Self::has_child_process() {
            transports.push(TransportType::ChildProcess);
        }

        transports
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_detection() {
        let transports = Features::available_transports();
        assert!(
            !transports.is_empty(),
            "At least one transport should be available"
        );

        // stdio should always be available in default configuration
        assert!(Features::has_stdio());
    }
}
