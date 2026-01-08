//! # TurboMCP WebSocket Transport
//!
//! WebSocket bidirectional transport implementation for the TurboMCP SDK.
//! This crate provides full MCP 2025-06-18 protocol support with bidirectional
//! communication, server-initiated requests, and elicitation handling.
//!
//! ## Features
//!
//! - **Bidirectional Communication**: Full request-response patterns with correlation
//! - **Elicitation Support**: Server-initiated requests with timeout handling
//! - **Automatic Reconnection**: Configurable exponential backoff retry logic
//! - **Keep-Alive**: Periodic ping/pong to maintain connections
//! - **Compression**: Optional message compression support
//! - **TLS Support**: Secure WebSocket connections (WSS)
//! - **Metrics Collection**: Comprehensive transport metrics and monitoring
//! - **Background Tasks**: Efficient management of concurrent operations
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use turbomcp_websocket::{WebSocketBidirectionalTransport, WebSocketBidirectionalConfig};
//! use turbomcp_transport_traits::Transport;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create client configuration
//! let config = WebSocketBidirectionalConfig::client("ws://localhost:8080".to_string())
//!     .with_max_concurrent_elicitations(5)
//!     .with_compression(true);
//!
//! // Create and connect transport
//! let transport = WebSocketBidirectionalTransport::new(config).await?;
//! transport.connect().await?;
//!
//! // Use the transport...
//! # Ok(())
//! # }
//! ```
//!
//! ## Architecture
//!
//! The WebSocket transport is organized into focused components:
//!
//! ```text
//! turbomcp-websocket/
//! ├── config.rs        # Configuration types and builders
//! ├── types.rs         # Core types and type aliases
//! ├── connection.rs    # Connection management and lifecycle
//! ├── tasks.rs         # Background task management
//! ├── elicitation.rs   # Elicitation handling and timeout management
//! ├── mcp_methods.rs   # MCP protocol method implementations
//! ├── transport.rs     # Main Transport trait implementation
//! └── bidirectional.rs # BidirectionalTransport trait implementation
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
    clippy::missing_errors_doc,
    clippy::must_use_candidate
)]

pub mod bidirectional;
pub mod config;
pub mod connection;
pub mod elicitation;
pub mod mcp_methods;
pub mod tasks;
pub mod transport;
pub mod types;

// Re-export main types for convenience
pub use bidirectional::CorrelationInfo;
pub use config::{ReconnectConfig, TlsConfig, WebSocketBidirectionalConfig};
pub use elicitation::ElicitationInfo;
pub use transport::TransportStatus;
pub use types::{
    MessageProcessingResult, PendingElicitation, WebSocketBidirectionalTransport,
    WebSocketConnectionStats, WebSocketStreamHandler,
};

// Re-export transport traits for convenience
pub use turbomcp_transport_traits::{
    BidirectionalTransport, ConnectionState, CorrelationContext, MessageDirection, Transport,
    TransportCapabilities, TransportConfig, TransportError, TransportEvent, TransportEventEmitter,
    TransportMessage, TransportMessageMetadata, TransportMetrics, TransportResult, TransportState,
    TransportType,
};
