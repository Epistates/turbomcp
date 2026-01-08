//! # TurboMCP HTTP Transport
//!
//! MCP 2025-11-25 compliant HTTP/SSE client transport implementation.
//!
//! This crate provides the HTTP client transport for the Model Context Protocol (MCP),
//! implementing the Streamable HTTP transport specification with full SSE support.
//!
//! ## Features
//!
//! - **MCP 2025-11-25 Specification Compliance**: Full implementation of the streamable HTTP spec
//! - **Single Endpoint Design**: All communication through one MCP endpoint
//! - **SSE Support**: Server-Sent Events for server-to-client streaming
//! - **Endpoint Discovery**: Automatic endpoint URI discovery via SSE events
//! - **Session Management**: Mcp-Session-Id header support for session tracking
//! - **Auto-Reconnect**: Configurable retry policies with exponential backoff
//! - **Last-Event-ID Resumability**: Resume SSE streams from last received event
//! - **TLS 1.3**: Minimum TLS version enforcement for security
//! - **Size Limits**: Configurable request/response size validation
//!
//! ## Usage
//!
//! ```rust,no_run
//! use turbomcp_http::{StreamableHttpClientTransport, StreamableHttpClientConfig};
//! use turbomcp_transport_traits::Transport;
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
//!     let transport = StreamableHttpClientTransport::new(config);
//!     transport.connect().await?;
//!
//!     // Transport is ready for MCP communication
//!     println!("HTTP transport connected");
//!     Ok(())
//! }
//! ```
//!
//! ## MCP Protocol Flow
//!
//! 1. Client connects and optionally opens SSE stream (GET with Accept: text/event-stream)
//! 2. Server may send "endpoint" event with message endpoint URI
//! 3. Client sends requests via POST to message endpoint
//! 4. Server responds with JSON or SSE stream (Accept header negotiation)
//! 5. Client terminates session with DELETE request
//!
//! ## Configuration Options
//!
//! ```rust
//! use turbomcp_http::{StreamableHttpClientConfig, RetryPolicy};
//! use turbomcp_transport_traits::{LimitsConfig, TlsConfig};
//! use std::time::Duration;
//!
//! let config = StreamableHttpClientConfig {
//!     base_url: "https://api.example.com".to_string(),
//!     endpoint_path: "/mcp".to_string(),
//!     timeout: Duration::from_secs(30),
//!     retry_policy: RetryPolicy::Exponential {
//!         base: Duration::from_secs(1),
//!         max_delay: Duration::from_secs(60),
//!         max_attempts: Some(10),
//!     },
//!     auth_token: Some("your-token".to_string()),
//!     limits: LimitsConfig::default(),
//!     tls: TlsConfig::modern(),
//!     ..Default::default()
//! };
//! ```
//!
//! ## Security
//!
//! - TLS 1.3 is required by default (v3.0 security requirement)
//! - Certificate validation is enabled by default
//! - Disabling certificate validation requires explicit environment variable opt-in
//! - Request/response size limits prevent resource exhaustion attacks

#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub,
    clippy::all
)]
#![deny(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod transport;

// Re-export the transport implementation
pub use transport::{RetryPolicy, StreamableHttpClientConfig, StreamableHttpClientTransport};

// Re-export common types from traits crate for convenience
pub use turbomcp_transport_traits::{
    LimitsConfig, TlsConfig, TlsVersion, Transport, TransportCapabilities, TransportError,
    TransportMessage, TransportMetrics, TransportResult, TransportState, TransportType,
};
