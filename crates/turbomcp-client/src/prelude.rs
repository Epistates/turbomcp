//! Prelude module for convenient imports
//!
//! This module re-exports the most commonly used types for building
//! applications with the TurboMCP client library.
//!
//! # Example
//!
//! ```rust,no_run
//! use turbomcp_client::prelude::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // All common types are available without deep imports
//!     let client = Client::new(StdioTransport::new());
//!     client.initialize().await?;
//!
//!     let tools = client.list_tools().await?;
//!     println!("Found {} tools", tools.len());
//!
//!     Ok(())
//! }
//! ```

pub use crate::{
    // Core client types
    Client,
    ClientBuilder,
    ClientCapabilities,
    ConnectionConfig,
    InitializeResult,

    // Result/Error (most commonly used) - re-exported from turbomcp_protocol
    Result,  // Note: This is Result<T, Box<Error>> from protocol
    Error,

    // Handlers (bidirectional communication)
    ElicitationHandler,
    ElicitationRequest,
    ElicitationResponse,
    ElicitationAction,
    ProgressHandler,
    ProgressNotification,
    LogHandler,
    LogMessage,
    ResourceUpdateHandler,
    ResourceUpdateNotification,
    ResourceChangeType,
    RootsHandler,
    HandlerError,
    HandlerResult,

    // Sampling
    SamplingHandler,
    ServerInfo,
    UserInteractionHandler,

    // Plugin system
    ClientPlugin,
    PluginConfig,
    PluginContext,
    PluginResult,
    PluginError,
    MetricsPlugin,
    RetryPlugin,
    CachePlugin,
};

// Transport re-exports (with feature gates - must be separate items)
#[cfg(feature = "stdio")]
pub use crate::StdioTransport;

#[cfg(feature = "http")]
pub use crate::HttpTransport;

#[cfg(feature = "tcp")]
pub use crate::TcpTransport;

#[cfg(feature = "unix")]
pub use crate::UnixTransport;

#[cfg(feature = "websocket")]
pub use crate::WebSocketBidirectionalTransport;

// Re-export commonly used protocol types
pub use turbomcp_protocol::types::{
    // Core types
    Tool,
    Prompt,
    Resource,
    ResourceContents,

    // Messaging
    CreateMessageRequest,
    CreateMessageResult,
    Content,
    TextContent,
    ImageContent,
    EmbeddedResource,
    Role,
    StopReason,

    // Logging
    LogLevel,

    // Progress
    ProgressNotification as ProtocolProgressNotification,
    ProgressToken,

    // Completion
    CompletionContext,
    CompleteResult,

    // Roots
    Root,
};

// Re-export async-trait for handler implementations
pub use async_trait::async_trait;

// Re-export Arc for handler registration
pub use std::sync::Arc;
