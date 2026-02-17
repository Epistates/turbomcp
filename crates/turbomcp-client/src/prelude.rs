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

// Version information
pub use crate::{CRATE_NAME, VERSION};

pub use crate::{
    CancellationHandler,
    CancelledNotification,
    // Core client types
    Client,
    ClientBuilder,
    ClientCapabilities,
    ConnectionConfig,
    ElicitationAction,
    // Handlers (bidirectional communication)
    ElicitationHandler,
    ElicitationRequest,
    ElicitationResponse,
    Error,

    HandlerError,
    HandlerResult,

    InitializeResult,

    LogHandler,
    LoggingNotification,
    PromptListChangedHandler,
    ResourceListChangedHandler,
    ResourceUpdateHandler,
    ResourceUpdatedNotification,
    // Result/Error (most commonly used) - re-exported from turbomcp_protocol
    Result, // Note: This is Result<T, Box<Error>> from protocol
    RootsHandler,
    // Sampling
    SamplingHandler,
    ServerInfo,
    ToolListChangedHandler,
    UserInteractionHandler,
};

// Transport re-exports (with feature gates - must be separate items)
#[cfg(feature = "stdio")]
pub use crate::StdioTransport;

#[cfg(feature = "http")]
pub use crate::{RetryPolicy, StreamableHttpClientConfig, StreamableHttpClientTransport};

#[cfg(feature = "tcp")]
pub use crate::{TcpTransport, TcpTransportBuilder};

#[cfg(feature = "unix")]
pub use crate::{UnixTransport, UnixTransportBuilder};

#[cfg(feature = "websocket")]
pub use crate::{WebSocketBidirectionalConfig, WebSocketBidirectionalTransport};

// Re-export commonly used protocol types
pub use turbomcp_protocol::types::{
    CompleteResult,

    // Completion
    CompletionContext,
    Content,
    // Messaging
    CreateMessageRequest,
    CreateMessageResult,
    EmbeddedResource,
    ImageContent,
    // Logging
    LogLevel,

    Prompt,
    Resource,
    ResourceContents,

    Role,
    // Roots
    Root,
    StopReason,

    TextContent,
    // Core types
    Tool,
};

// Re-export Transport trait for generic bounds (e.g., LLM framework integrations)
pub use turbomcp_transport::Transport;

// Re-export Arc for handler registration
pub use std::sync::Arc;
