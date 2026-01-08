//! # TurboMCP Unix Domain Socket Transport
//!
//! Unix domain socket transport implementation for the TurboMCP SDK.
//!
//! This crate provides inter-process communication over Unix domain sockets with:
//!
//! - **Server Mode**: Accept multiple client connections with automatic handling
//! - **Client Mode**: Connect to a Unix socket server
//! - **Bidirectional Communication**: Full-duplex message exchange
//! - **Backpressure Handling**: Bounded channels prevent memory exhaustion
//! - **Graceful Shutdown**: Clean task termination and socket cleanup
//! - **Message Framing**: Uses LinesCodec for reliable newline-delimited JSON
//!
//! ## Quick Start
//!
//! ### Server Mode
//!
//! ```rust,ignore
//! use turbomcp_unix::{UnixTransport, UnixTransportBuilder};
//! use turbomcp_transport_traits::Transport;
//! use std::path::PathBuf;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let transport = UnixTransportBuilder::new_server()
//!         .socket_path("/tmp/my-mcp.sock")
//!         .permissions(0o600)
//!         .build();
//!
//!     transport.connect().await?; // Starts listening
//!     Ok(())
//! }
//! ```
//!
//! ### Client Mode
//!
//! ```rust,ignore
//! use turbomcp_unix::{UnixTransport, UnixTransportBuilder};
//! use turbomcp_transport_traits::Transport;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let transport = UnixTransportBuilder::new_client()
//!         .socket_path("/tmp/my-mcp.sock")
//!         .build();
//!
//!     transport.connect().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## v3.0 Modular Architecture
//!
//! This crate is part of TurboMCP v3.0's modular transport architecture:
//!
//! - **Foundation**: `turbomcp-transport-traits` provides core abstractions
//! - **Individual Transports**: Each transport (stdio, http, websocket, tcp, unix) is a separate crate
//! - **Backward Compatibility**: `turbomcp-transport` re-exports all transports

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
    clippy::must_use_candidate,
    clippy::return_self_not_must_use,
    clippy::missing_panics_doc,
    clippy::default_trait_access
)]

mod transport;

pub use transport::{UnixConfig, UnixTransport, UnixTransportBuilder};

// Re-export transport traits for convenience
pub use turbomcp_transport_traits::{
    AtomicMetrics, Transport, TransportCapabilities, TransportError, TransportMessage,
    TransportMetrics, TransportResult, TransportState, TransportType,
};
