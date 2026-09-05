//! TurboMCP v4 client.
//!
//! The other half of the protocol: a [`Client`] drives an MCP server over any
//! [`Transport`](turbomcp_service::Transport), speaking version-stable
//! [`neutral`](turbomcp_protocol::neutral) types while handling version
//! negotiation and wire decoding internally.
//!
//! - [`Connection`] is the raw transport + request/response correlation.
//! - [`Client`] / [`ClientBuilder`] add the handshake, the negotiated
//!   [`ConnectMode`], modern `_meta` stamping, and the typed MCP API.
//! - [`connect_child`] spawns a server subprocess and connects over its stdio.
#![forbid(unsafe_code)]
// docs.rs builds with `--cfg docsrs` on nightly so every feature-gated item
// renders with the feature that unlocks it.
#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs)]

mod cache;
mod client;
mod connection;
mod error;
mod handler;
mod stdio;

pub use client::{Client, ClientBuilder, ConnectMode};
pub use connection::{Connection, DEFAULT_REQUEST_TIMEOUT};
pub use error::{ClientError, ClientResult};
pub use handler::ClientHandler;
pub use stdio::connect_child;

/// Re-exported so implementers of [`ClientHandler`] can write
/// `#[async_trait]` without taking a direct dependency on the crate.
pub use async_trait::async_trait;

#[cfg(feature = "http")]
#[cfg_attr(docsrs, doc(cfg(feature = "http")))]
mod http;
#[cfg(feature = "http")]
#[cfg_attr(docsrs, doc(cfg(feature = "http")))]
pub use http::{BearerSource, HttpClientError, HttpClientTransport, connect_http};
