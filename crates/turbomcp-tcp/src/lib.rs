//! # TurboMCP TCP Transport
//!
//! TCP socket transport implementation for the TurboMCP SDK.
//! This crate provides newline-delimited JSON-RPC communication over TCP sockets.
//!
//! ## Features
//!
//! - **Server Mode**: Accept multiple client connections with automatic handling
//! - **Client Mode**: Connect to a remote TCP server
//! - **Bidirectional Communication**: Full-duplex message exchange
//! - **Backpressure Handling**: Bounded channels prevent memory exhaustion
//! - **Graceful Shutdown**: Clean task termination on disconnect
//! - **Message Framing**: Uses LinesCodec for reliable newline-delimited JSON
//!
//! ## Quick Start
//!
//! ### Server Mode
//!
//! ```rust,ignore
//! use turbomcp_tcp::{TcpTransport, TcpTransportBuilder};
//! use turbomcp_transport_traits::Transport;
//! use std::net::SocketAddr;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let addr: SocketAddr = "127.0.0.1:8080".parse()?;
//!     let transport = TcpTransportBuilder::new()
//!         .bind_addr(addr)
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
//! use turbomcp_tcp::{TcpTransport, TcpTransportBuilder};
//! use turbomcp_transport_traits::Transport;
//! use std::net::SocketAddr;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let bind_addr: SocketAddr = "127.0.0.1:0".parse()?;
//!     let remote_addr: SocketAddr = "127.0.0.1:8080".parse()?;
//!
//!     let transport = TcpTransportBuilder::new()
//!         .bind_addr(bind_addr)
//!         .remote_addr(remote_addr)
//!         .build();
//!
//!     transport.connect().await?;
//!     Ok(())
//! }
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

mod transport;

pub use transport::{TcpConfig, TcpTransport, TcpTransportBuilder};

// Re-export transport traits for convenience
pub use turbomcp_transport_traits::{
    AtomicMetrics, Transport, TransportCapabilities, TransportError, TransportMessage,
    TransportMetrics, TransportResult, TransportState, TransportType,
};
