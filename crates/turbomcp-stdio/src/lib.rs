//! # TurboMCP STDIO Transport
//!
//! Standard I/O transport implementation for the TurboMCP Model Context Protocol SDK.
//! This transport uses stdin/stdout for communication, which is the standard way
//! MCP servers communicate with clients.
//!
//! ## MCP Specification Compliance (2025-06-18)
//!
//! This implementation is **fully compliant** with the MCP stdio transport specification:
//!
//! - **Newline-delimited JSON**: Uses `LinesCodec` for proper message framing
//! - **No embedded newlines**: Validates messages don't contain `\n` or `\r` characters
//! - **UTF-8 encoding**: All messages are UTF-8 encoded (enforced by `std::str::from_utf8`)
//! - **stderr for logging**: Uses `tracing` crate which outputs to stderr by default
//! - **Bidirectional communication**: Supports both client→server and server→client messages
//! - **Valid JSON only**: Validates all messages are well-formed JSON before sending
//!
//! Per MCP spec: "Messages are delimited by newlines, and **MUST NOT** contain embedded newlines."
//!
//! ## Usage
//!
//! ```rust,ignore
//! use turbomcp_stdio::StdioTransport;
//! use turbomcp_transport_traits::Transport;
//!
//! #[tokio::main]
//! async fn main() {
//!     let transport = StdioTransport::new();
//!     transport.connect().await.unwrap();
//!
//!     // Send and receive messages...
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

pub use transport::{StdioTransport, StdioTransportFactory};

// Re-export common types for convenience
pub use turbomcp_transport_traits::{
    Transport, TransportCapabilities, TransportConfig, TransportError, TransportFactory,
    TransportMessage, TransportMetrics, TransportResult, TransportState, TransportType,
};
