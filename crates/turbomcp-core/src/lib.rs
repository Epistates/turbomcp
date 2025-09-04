//! # TurboMCP Core
//!
//! Foundation crate for the Model Context Protocol (MCP) SDK providing core types,
//! error handling, and optimized abstractions for building MCP applications.
//!
//! ## Features
//!
//! - **SIMD-Accelerated JSON** - Fast processing with `simd-json` and `sonic-rs`
//! - **Rich Error Handling** - Comprehensive error types with context information
//! - **Session Management** - Configurable LRU eviction and lifecycle management
//! - **Zero-Copy Processing** - Memory-efficient message handling with `Bytes`
//! - **Request Context** - Full request/response context tracking for observability
//! - **Server Capabilities** - Support for server-initiated requests (sampling, elicitation)
//! - **Performance Optimized** - Memory-bounded state management with cleanup tasks
//! - **Observability Ready** - Built-in support for tracing and metrics collection
//!
//! ## Architecture
//!
//! ```text
//! turbomcp-core/
//! ├── error/          # Error types and handling
//! ├── message/        # Message types and serialization
//! ├── types/          # Core protocol types
//! ├── context/        # Request/response context with server capabilities
//! ├── session/        # Session management
//! ├── registry/       # Component registry
//! ├── state/          # State management
//! └── utils/          # Utility functions
//! ```
//!
//! ## Server Capabilities
//!
//! The core provides a `ServerCapabilities` trait that enables server-initiated requests
//! to clients, supporting bidirectional communication patterns like sampling and elicitation:
//!
//! ```rust,no_run
//! use turbomcp_core::{RequestContext, ServerCapabilities};
//!
//! // Tools can access server capabilities through the context
//! async fn my_tool(ctx: RequestContext) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
//!     if let Some(capabilities) = ctx.server_capabilities() {
//!         // Make a sampling request to the client
//!         let request = serde_json::json!({
//!             "messages": [{"role": "user", "content": "Hello"}],
//!             "max_tokens": 100
//!         });
//!         let response = capabilities.create_message(request).await?;
//!     }
//!     Ok(())
//! }
//! ```
//!
//! ## Usage
//!
//! This crate provides the foundation types and utilities used by other `TurboMCP` crates.
//! It is typically not used directly but imported by the main `turbomcp` SDK.

#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub,
    clippy::all
)]
#![cfg_attr(
    all(not(feature = "mmap"), not(feature = "lock-free")),
    deny(unsafe_code)
)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![allow(
    clippy::module_name_repetitions,
    clippy::missing_errors_doc,  // Error documentation in progress
    clippy::cast_possible_truncation,  // Intentional in metrics/performance code
    clippy::cast_possible_wrap,  // Intentional in metrics/performance code  
    clippy::cast_precision_loss,  // Intentional for f64 metrics
    clippy::cast_sign_loss,  // Intentional for metrics
    clippy::must_use_candidate,  // Too pedantic for library APIs
    clippy::return_self_not_must_use,  // Constructor methods don't need must_use
    clippy::struct_excessive_bools,  // Sometimes bools are the right design
    clippy::missing_panics_doc,  // Panic docs added where genuinely needed
    clippy::default_trait_access,  // Default::default() is sometimes clearer
    clippy::significant_drop_tightening,  // Overly pedantic about drop timing
    clippy::used_underscore_binding  // Sometimes underscore bindings are needed
)]

pub mod context;
pub mod enhanced_registry;
pub mod error;
pub mod error_utils;
pub mod handlers;
pub mod lock_free;
pub mod message;
pub mod registry;
pub mod session;
pub mod state;
pub mod types;
pub mod utils;
pub mod zero_copy;

#[cfg(feature = "fancy-errors")]
pub mod config;

// Re-export commonly used types
pub use context::{
    BidirectionalContext, ClientCapabilities, ClientId, ClientIdExtractor, ClientSession,
    CommunicationDirection, CommunicationInitiator, CompletionCapabilities, CompletionContext,
    CompletionOption, CompletionReference, ConnectionMetrics, ElicitationContext, ElicitationState,
    PingContext, PingOrigin, RequestContext, RequestContextExt, RequestInfo,
    ResourceTemplateContext, ResponseContext, ServerCapabilities, ServerInitiatedContext,
    ServerInitiatedType, TemplateParameter,
};
pub use enhanced_registry::{EnhancedRegistry, HandlerStats};
pub use error::{Error, ErrorKind, Result};
pub use handlers::{
    CompletionItem, CompletionProvider, ElicitationHandler, ElicitationResponse,
    HandlerCapabilities, PingHandler, PingResponse, ResolvedResource, ResourceTemplate,
    ResourceTemplateHandler, ServerInitiatedCapabilities, TemplateParam,
};
pub use message::{Message, MessageId, MessageMetadata};
pub use session::{SessionAnalytics, SessionConfig, SessionManager};
pub use state::StateManager;
pub use types::{ContentType, ProtocolVersion, Timestamp};

/// Alias for RequestContext for backward compatibility
pub type Context = RequestContext;

/// Current MCP protocol version supported by this SDK
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// Supported protocol versions for compatibility
pub const SUPPORTED_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// Maximum message size in bytes (16MB)
pub const MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;

/// Default timeout for operations in milliseconds
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// SDK version information
pub const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");

/// SDK name identifier
pub const SDK_NAME: &str = "turbomcp";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_constants() {
        // These constants are compile-time defined and should never be empty
        assert!(SUPPORTED_VERSIONS.contains(&PROTOCOL_VERSION));
    }

    #[test]
    fn test_size_constants() {
        // Constants are statically verified at compile-time
        // These tests document our design constraints

        // Verify message size is reasonable for MCP protocol
        const _: () = assert!(
            MAX_MESSAGE_SIZE > 1024,
            "MAX_MESSAGE_SIZE must be larger than 1KB"
        );
        const _: () = assert!(
            MAX_MESSAGE_SIZE == 16 * 1024 * 1024,
            "MAX_MESSAGE_SIZE must be 16MB"
        );

        // Verify timeout allows for reasonable operation completion
        const _: () = assert!(
            DEFAULT_TIMEOUT_MS > 1000,
            "DEFAULT_TIMEOUT_MS must be larger than 1 second"
        );
        const _: () = assert!(
            DEFAULT_TIMEOUT_MS == 30_000,
            "DEFAULT_TIMEOUT_MS must be 30 seconds"
        );
    }
}
