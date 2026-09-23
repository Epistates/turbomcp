//! `TurboMCP` gRPC Transport
//!
//! High-performance gRPC transport for the Model Context Protocol (MCP).
//! Built on [tonic](https://github.com/hyperium/tonic) for async/await support
//! and full HTTP/2 capabilities.
//!
//! # Features
//!
//! - **Server**: gRPC server implementation with streaming notifications
//! - **Client**: gRPC client wrapper with explicit session initialization
//! - **Tower Integration**: Composable middleware via Tower
//! - **TLS** (`tls` feature): rustls through tonic; configure the server with
//!   `Server::tls_config`, and the client verifies `https://` endpoints against
//!   the platform's root certificates
//!
//! Building needs the `protoc` Protocol Buffers compiler (on `PATH`, or named
//! by `PROTOC`) to compile `src/proto/mcp.proto`.
//!
//! # Quick Start
//!
//! ## Server
//!
//! ```ignore
//! use turbomcp_grpc::server::McpGrpcServer;
//! use turbomcp_types::Tool;
//!
//! let server = McpGrpcServer::builder()
//!     .add_tool(Tool { name: "hello".into(), ..Default::default() })
//!     // .tool_handler(MyHandler) // implements ToolHandler
//!     .build();
//!
//! tonic::transport::Server::builder()
//!     .add_service(server.into_service())
//!     .serve("[::1]:50051".parse()?)
//!     .await?;
//! ```
//!
//! ## Client
//!
//! ```ignore
//! use turbomcp_grpc::client::McpGrpcClient;
//!
//! let mut client = McpGrpcClient::connect("http://[::1]:50051").await?;
//! client.initialize().await?;
//! let result = client
//!     .call_tool("hello", Some(serde_json::json!({"name": "World"})))
//!     .await?;
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
