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
//!
//! The client speaks to any [`Transport`](turbomcp_service::Transport). The
//! Streamable HTTP one is `turbomcp-transport-http`'s `HttpClientTransport`
//! (feature `client`), next to the server half of the same protocol.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod cache;
mod client;
mod connection;
mod error;
mod extension;
mod handler;
mod observe;
mod options;
mod progress;
mod stdio;
mod subscription;
mod task;

pub use client::{Client, ClientBuilder, ConnectMode};
pub use connection::{Connection, DEFAULT_REQUEST_TIMEOUT};
pub use error::{ClientError, ClientResult};
pub use extension::ClientExtension;
pub use handler::{
    ClientHandlers, ElicitationHandler, NotificationHandler, RootsHandler, SamplingHandler,
};
pub use observe::{ClosedSession, OutboundRequest, RequestObserver, RequestScope};
pub use options::CallOptions;
pub use progress::ProgressCallback;
pub use stdio::connect_child;
pub use subscription::{Subscription, SubscriptionEnd, SubscriptionEvent};
pub use task::{Detached, TaskInfo, TaskPage, TaskStatus, ToolTask};

/// Re-exported so implementers of [`ElicitationHandler`] can write
/// `#[async_trait]` without taking a direct dependency on the crate.
pub use async_trait::async_trait;
