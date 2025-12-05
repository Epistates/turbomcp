//! # TurboMCP Transport
//!
//! Transport layer implementations for the Model Context Protocol with runtime
//! selection, fault tolerance, and multiple protocol support.
//!
//! ## Supported Transports
//!
//! - **STDIO**: Standard input/output for command-line MCP servers (always available)
//! - **TCP**: Direct TCP socket communication for network deployments
//! - **Unix Sockets**: Fast local inter-process communication
//! - **HTTP/SSE**: HTTP with Server-Sent Events for server push
//! - **WebSocket Bidirectional**: Full-duplex communication for elicitation
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
//! └── metrics/        # Transport performance metrics
//! ```
//!
//! ## Usage Examples
//!
//! ### WebSocket Bidirectional Transport
//!
//! ```rust,no_run
//! # #[cfg(feature = "websocket")]
//! # {
//! use turbomcp_transport::{WebSocketBidirectionalTransport, WebSocketBidirectionalConfig};
//! use std::time::Duration;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = WebSocketBidirectionalConfig {
//!         url: Some("ws://localhost:8080".to_string()),
//!         max_concurrent_elicitations: 10,
//!         elicitation_timeout: Duration::from_secs(60),
//!         keep_alive_interval: Duration::from_secs(30),
//!         reconnect: Default::default(),
//!         ..Default::default()
//!     };
//!
//!     let transport = WebSocketBidirectionalTransport::new(config).await?;
//!     
//!     // Transport is ready for bidirectional communication
//!     println!("WebSocket transport established");
//!     Ok(())
//! }
//! # }
//! ```
//!
//! ### MCP 2025-06-18 Streamable HTTP (Client)
//!
//! ```rust,no_run
//! # #[cfg(feature = "http")]
//! # {
//! use turbomcp_transport::streamable_http_client::{StreamableHttpClientConfig, StreamableHttpClientTransport};
//! use std::time::Duration;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = StreamableHttpClientConfig {
//!         base_url: "http://localhost:8080".to_string(),
//!         endpoint_path: "/mcp".to_string(),
//!         timeout: Duration::from_secs(30),
//!         ..Default::default()
//!     };
//!
//!     let mut transport = StreamableHttpClientTransport::new(config);
//!     // Full MCP 2025-06-18 compliance with SSE support
//!     Ok(())
//! }
//! # }
//! ```
//!
//! ### Runtime Transport Selection
//!
//! ```rust,no_run
//! use turbomcp_transport::Features;
//!
//! // Check available transports at runtime
//! if Features::has_websocket() {
//!     println!("WebSocket transport available");
//! }
//!
//! if Features::has_http() {
//!     println!("HTTP transport available");
//! }
//!
//! // Always available
//! assert!(Features::has_stdio());
//!
//! // Get list of all available transports
//! let available = Features::available_transports();
//! println!("Available transports: {:?}", available);
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

/// Bidirectional transport wrappers and utilities.
pub mod bidirectional;
/// Core transport traits, types, and errors.
pub mod core;

// MCP 2025-06-18 Compliant Streamable HTTP Transport (Recommended)
/// HTTP transport types and configuration for MCP 2025-06-18 specification compliance.
///
/// This module provides configuration and session management types.
/// The actual HTTP server implementation is in `turbomcp_server::runtime::http`.
#[cfg(feature = "http")]
#[cfg_attr(docsrs, doc(cfg(feature = "http")))]
pub mod streamable_http;

/// Backwards-compatible re-export of `streamable_http` module.
///
/// **Deprecated**: Use `streamable_http` instead. This alias will be removed in v3.0.0.
#[cfg(feature = "http")]
#[cfg_attr(docsrs, doc(cfg(feature = "http")))]
#[deprecated(since = "2.4.0", note = "Use `streamable_http` instead")]
pub mod streamable_http_v2 {
    pub use crate::streamable_http::*;
}

/// A streamable HTTP client transport implementation.
#[cfg(feature = "http")]
#[cfg_attr(docsrs, doc(cfg(feature = "http")))]
pub mod streamable_http_client;

/// Standard I/O (stdio) transport for command-line applications.
#[cfg(feature = "stdio")]
pub mod stdio;

// Tower service integration
/// Integration with the Tower service abstraction.
pub mod tower;

/// Integration with the Axum web framework.
#[cfg(feature = "http")]
pub mod axum;

/// WebSocket bidirectional transport for full-duplex communication with MCP 2025-06-18 compliance.
#[cfg(feature = "websocket")]
pub mod websocket_bidirectional;

/// TCP socket transport for network communication.
#[cfg(feature = "tcp")]
pub mod tcp;

/// Unix domain socket transport for inter-process communication.
#[cfg(feature = "unix")]
pub mod unix;

/// Transport for managing child processes.
pub mod child_process;

// Server-specific transport functionality
/// Server-side transport management and dispatch.
pub mod server;

/// Message compression utilities.
#[cfg(feature = "compression")]
pub mod compression;

/// Transport configuration builders and types.
pub mod config;
/// Metrics and performance monitoring for transports.
pub mod metrics;
/// Resilience patterns like circuit breakers and retries.
pub mod resilience;
/// Security features for transports, including authentication and rate limiting.
pub mod security;
/// Utilities for shared transport instances.
pub mod shared;

// Re-export bidirectional transport functionality
pub use bidirectional::{
    BidirectionalTransportWrapper, ConnectionState, CorrelationContext, MessageDirection,
    MessageRouter, ProtocolDirectionValidator, RouteAction,
};

// Re-export core transport traits and types
pub use core::{
    BidirectionalTransport, StreamingTransport, Transport, TransportCapabilities, TransportConfig,
    TransportError, TransportEvent, TransportMessage, TransportMetrics, TransportResult,
    TransportState, TransportType, validate_request_size, validate_response_size,
};

// Re-export server transport functionality
pub use server::{
    ServerTransportConfig, ServerTransportConfigBuilder, ServerTransportDispatcher,
    ServerTransportEvent, ServerTransportManager, ServerTransportWrapper,
};

// Re-export transport implementations
#[cfg(feature = "stdio")]
pub use stdio::StdioTransport;

// Re-export Tower integration
pub use tower::{SessionInfo, SessionManager, TowerTransportAdapter};

// Re-export Axum integration
#[cfg(feature = "http")]
pub use axum::{AxumMcpExt, McpAppState, McpServerConfig, McpService};

#[cfg(feature = "websocket")]
pub use websocket_bidirectional::{
    ReconnectConfig, TlsConfig, WebSocketBidirectionalConfig, WebSocketBidirectionalTransport,
};

#[cfg(feature = "tcp")]
pub use tcp::TcpTransport;

#[cfg(feature = "unix")]
pub use unix::UnixTransport;

// Re-export child process transport (always available)
pub use child_process::{ChildProcessConfig, ChildProcessTransport};

// Re-export utilities
pub use config::{LimitsConfig, TransportConfigBuilder};
pub use resilience::{
    CircuitBreakerConfig, CircuitBreakerStats, CircuitState, HealthCheckConfig, HealthInfo,
    HealthStatus, RetryConfig, TurboTransport,
};
pub use security::{
    AuthConfig, AuthMethod, EnhancedSecurityConfigBuilder, OriginConfig, RateLimitConfig,
    RateLimiter, SecureSessionInfo, SecurityConfigBuilder, SecurityError, SecurityValidator,
    SessionSecurityConfig, SessionSecurityManager, validate_message_size,
};
pub use shared::SharedTransport;

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
