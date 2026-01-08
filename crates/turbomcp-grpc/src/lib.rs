//! `TurboMCP` gRPC Transport
//!
//! High-performance gRPC transport for the Model Context Protocol (MCP).
//! Built on [tonic](https://github.com/hyperium/tonic) for async/await support
//! and full HTTP/2 capabilities.
//!
//! # Features
//!
//! - **Server**: gRPC server implementation with streaming notifications
//! - **Client**: gRPC client with automatic reconnection
//! - **Tower Integration**: Composable middleware via Tower
//! - **TLS**: Optional TLS 1.3 support via rustls
//!
//! # Quick Start
//!
//! ## Server
//!
//! ```ignore
//! use turbomcp_grpc::server::McpGrpcServer;
//!
//! let server = McpGrpcServer::builder()
//!     .add_tool("hello", |name: String| async move {
//!         Ok(format!("Hello, {}!", name))
//!     })
//!     .build()?;
//!
//! server.serve("[::1]:50051").await?;
//! ```
//!
//! ## Client
//!
//! ```ignore
//! use turbomcp_grpc::client::McpGrpcClient;
//!
//! let client = McpGrpcClient::connect("http://[::1]:50051").await?;
//! let result = client.call_tool("hello", json!({"name": "World"})).await?;
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

/// Generated protobuf types for MCP
pub mod proto {
    #![allow(missing_docs)]
    #![allow(clippy::all)]
    #![allow(clippy::pedantic)]
    tonic::include_proto!("turbomcp.mcp.v1");
}

pub mod convert;
pub mod error;

#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
pub mod server;

#[cfg(feature = "client")]
#[cfg_attr(docsrs, doc(cfg(feature = "client")))]
pub mod client;

pub mod layer;

// Re-exports for convenience
pub use error::{GrpcError, GrpcResult};

#[cfg(feature = "server")]
pub use server::McpGrpcServer;

#[cfg(feature = "client")]
pub use client::McpGrpcClient;

pub use layer::McpGrpcLayer;
