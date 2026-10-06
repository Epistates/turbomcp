//! # turbomcp-proxy
//!
//! Remote MCP servers as local ones: the embeddable gateway for TurboMCP v4.
//!
//! A [`RemoteServer`] connects to an upstream (a command over stdio, a
//! Streamable HTTP or WebSocket endpoint) and implements the same capability
//! traits a `#[server]` does by forwarding to it. That makes it the same kind
//! of object as a local server, so the rest of the stack applies unchanged:
//!
//! - **Bridge** one server: serve it over another transport or revision
//!   (a stdio-only server on HTTP; a `2025-11-25` server to `2026-07-28`
//!   clients, and back).
//! - **Aggregate** many: mount remotes beside your own tools in a
//!   [`Composite`](turbomcp_server::Composite), each under a prefix or flat.
//! - **Govern** them: the composite's authentication, rate limits,
//!   per-caller visibility (a hidden upstream tool is indistinguishable from
//!   a nonexistent one), interceptors and telemetry cover remote components
//!   exactly as they cover local ones.
//!
//! ```no_run
//! use turbomcp_proxy::{RemoteServer, Upstream};
//! use turbomcp_server::Composite;
//! use turbomcp_core::Implementation;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let files = RemoteServer::connect(Upstream::stdio(
//!     "npx", ["-y", "@modelcontextprotocol/server-filesystem", "/srv/docs"],
//! ))
//! .await?;
//! let gateway = Composite::new(Implementation::new("gateway", "1.0.0"))
//!     .mount("files", files.clone().into_server())?
//!     .into_server();
//! # Ok(()) }
//! ```
//!
//! The proxy authenticates upstream as itself ([`OutboundAuth`]); the
//! downstream caller's token is never passed through. Cancellation, progress
//! and the caller's trace context cross the hop, and so do the upstream's
//! requests for input: its elicitation, sampling and roots requests reach the
//! downstream caller whose call caused them, on any pair of revisions.
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs)]

mod bridge;
mod connect;
mod error;
mod link;
mod pool;
mod process;
mod remote;
mod upstream;

pub use error::ProxyError;
pub use pool::UpstreamKey;
pub use remote::{RemoteServer, RemoteServerBuilder};
pub use upstream::{OutboundAuth, Upstream};
