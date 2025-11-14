//! # TurboMCP Protocol
//!
//! Complete Model Context Protocol (MCP) implementation in Rust, providing all protocol types,
//! traits, context management, and message handling for building MCP applications.
//!
//! ## What's Inside
//!
//! This crate provides everything needed for MCP:
//!
//! - **Types**: All MCP request/response types from the MCP 2025-06-18 specification
//! - **Traits**: `ServerToClientRequests` for bidirectional communication
//! - **Context**: Request and response context management with full observability
//! - **JSON-RPC**: JSON-RPC 2.0 implementation with batching and notifications
//! - **Validation**: JSON Schema validation with comprehensive constraints
//! - **Error Handling**: Rich error types with context and tracing
//! - **Message Handling**: Optimized message processing with zero-copy support
//! - **Session Management**: Configurable LRU eviction and lifecycle management
//! - **Zero-Copy**: Optional zero-copy optimizations for high performance
//!
//! ## Features
//!
//! ### Core Protocol Support
//! - Complete MCP 2025-06-18 protocol implementation
//! - JSON-RPC 2.0 support with batching and notifications
//! - Type-safe capability negotiation and compatibility checking
//! - Protocol versioning with backward compatibility
//! - Fast serialization with SIMD acceleration
//!
//! ### Advanced Protocol Features
//! - **Elicitation Protocol** - Server-initiated user input requests with rich schema validation
//! - **Sampling Support** - Bidirectional LLM sampling with fully-typed interfaces
//! - **Roots Protocol** - Filesystem boundaries with `roots/list` support
//! - **Server-to-Client Requests** - Fully typed trait for sampling, elicitation, and roots
//! - **Comprehensive Schema Builders** - Type-safe builders for all schema types
//!
//! ### Performance & Observability
//! - **SIMD-Accelerated JSON** - Fast processing with `simd-json` and `sonic-rs`
//! - **Zero-Copy Processing** - Memory-efficient message handling with `Bytes`
//! - **Request Context** - Full request/response context tracking for observability
//! - **Session Management** - Memory-bounded state management with cleanup tasks
//! - **Observability Ready** - Built-in support for tracing and metrics collection
//!
//! ## Migration from v1.x
//!
//! In v2.0.0, `turbomcp-core` was merged into `turbomcp-protocol` to eliminate circular
//! dependencies and enable fully-typed bidirectional communication.
//!
//! ```rust,ignore
//! // v1.x
//! use turbomcp_protocol::{RequestContext, Error};
//! use turbomcp_protocol::types::CreateMessageRequest;
//!
//! // v2.0.0
//! use turbomcp_protocol::{RequestContext, Error, types::CreateMessageRequest};
//! ```
//!
//! All functionality is preserved, just the import path changed!
//!
//! ## Architecture
//!
//! ```text
//! turbomcp-protocol/
//! ├── error/              # Error types and handling
//! ├── message/            # Message types and serialization
//! ├── context/            # Request/response context with server capabilities
//! ├── types/              # MCP protocol types
//! ├── jsonrpc/            # JSON-RPC 2.0 implementation
//! ├── validation/         # Schema validation
//! ├── session/            # Session management
//! ├── registry/           # Component registry
//! └── utils/              # Utility functions
//! ```
//!
//! ## Server-to-Client Communication
//!
//! The protocol provides a `ServerToClientRequests` trait that enables server-initiated requests
//! to clients, supporting bidirectional communication patterns like sampling and elicitation:
//!
//! ```rust,no_run
//! use turbomcp_protocol::{RequestContext, types::CreateMessageRequest, ServerToClientRequests};
//!
//! // Tools can access server capabilities through the context
//! async fn my_tool(ctx: RequestContext) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
//!     if let Some(capabilities) = ctx.clone().server_to_client() {
//!         // Make a fully-typed sampling request to the client
//!         let request = CreateMessageRequest {
//!             messages: vec![/* ... */],
//!             max_tokens: 100,
//!             model_preferences: None,
//!             system_prompt: None,
//!             include_context: None,
//!             temperature: None,
//!             stop_sequences: None,
//!             _meta: None,
//!         };
//!         let response = capabilities.create_message(request, ctx).await?;
//!     }
//!     Ok(())
//! }
//! ```

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
    clippy::used_underscore_binding,  // Sometimes underscore bindings are needed
    clippy::wildcard_imports  // Used in test modules
)]

// Core abstractions (merged from turbomcp-core in v2.0.0)
/// Configuration for protocol components.
pub mod config;
/// Request/response context, including server-to-client capabilities.
pub mod context;
/// An advanced handler registry with metrics and enhanced features.
pub mod enhanced_registry;
/// Error types and handling for the protocol.
pub mod error;
/// Utilities for creating and working with protocol errors.
pub mod error_utils;
/// Traits and types for handling different MCP requests (tools, prompts, etc.).
pub mod handlers;
/// Lock-free data structures for high-performance concurrent scenarios.
#[cfg(feature = "lock-free")]
pub mod lock_free;
/// Core message types and serialization logic.
pub mod message;
/// Basic handler registration and lookup.
pub mod registry;
/// Security-related utilities, such as path validation.
pub mod security;
/// Session management for client connections.
pub mod session;
/// Utilities for shared, concurrent state management.
pub mod shared;
/// State management for the protocol.
pub mod state;
/// General utility functions.
pub mod utils;
/// Zero-copy data handling utilities for performance-critical operations.
pub mod zero_copy;

// Protocol-specific modules
/// Capability negotiation and management.
pub mod capabilities;
// Old elicitation module removed - use types::elicitation instead (MCP 2025-06-18 compliant)
/// JSON-RPC 2.0 protocol implementation.
pub mod jsonrpc;
/// All MCP protocol types (requests, responses, and data structures).
pub mod types;
/// Schema validation for protocol messages.
pub mod validation;
/// Protocol version management and compatibility checking.
pub mod versioning;

// Test utilities (public to allow downstream crates to use them in tests)
// Following the pattern from axum and tokio
/// Public test utilities for use in downstream crates.
pub mod test_helpers;

// Re-export core types
pub use context::{
    BidirectionalContext, ClientCapabilities as ContextClientCapabilities, ClientId,
    ClientIdExtractor, ClientSession, CommunicationDirection, CommunicationInitiator,
    CompletionCapabilities, CompletionContext, CompletionOption,
    CompletionReference as ContextCompletionReference, ConnectionMetrics, ElicitationContext,
    ElicitationState, PingContext, PingOrigin, RequestContext, RequestContextExt, RequestInfo,
    ResourceTemplateContext, ResponseContext, ServerInitiatedContext, ServerInitiatedType,
    ServerToClientRequests, TemplateParameter,
};
// Timestamp and ContentType are now in types module
pub use enhanced_registry::{EnhancedRegistry, HandlerStats};
pub use error::{Error, ErrorContext, ErrorKind, Result, RetryInfo};
pub use handlers::{
    CompletionItem, CompletionProvider, ElicitationHandler, ElicitationResponse,
    HandlerCapabilities, JsonRpcHandler, PingHandler, PingResponse, ResolvedResource,
    ResourceTemplate as HandlerResourceTemplate, ResourceTemplateHandler, ServerInfo,
    ServerInitiatedCapabilities, TemplateParam,
};
pub use message::{Message, MessageId, MessageMetadata};
pub use registry::RegistryError;
pub use security::{validate_file_extension, validate_path, validate_path_within};
pub use session::{SessionAnalytics, SessionConfig, SessionManager};
pub use shared::{ConsumableShared, Shareable, Shared, SharedError};
pub use state::StateManager;

// Re-export ONLY essential types at root (v2.0 - improved ergonomics)
// Everything else requires module qualification: turbomcp_protocol::types::*
pub use types::{
    // Most common tool operations
    CallToolRequest,
    CallToolResult,

    ClientCapabilities,
    // Macro API types (used by generated code - not typically imported by users)
    GetPromptRequest,
    GetPromptResult,
    // Most common request/response pairs (initialization flow)
    InitializeRequest,
    InitializeResult,

    ReadResourceRequest,
    ReadResourceResult,

    // Capability negotiation (used in every initialize)
    ServerCapabilities,
};

// Note: types module is already declared as `pub mod types;` above
// Users access other types via turbomcp_protocol::types::Tool, etc.

// Allow deprecated on re-export - users importing JsonRpcBatch will still get deprecation warnings
#[allow(deprecated)]
pub use jsonrpc::{
    JsonRpcBatch, JsonRpcError, JsonRpcErrorCode, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, JsonRpcVersion,
};

pub use capabilities::{
    CapabilityMatcher, CapabilityNegotiator, CapabilitySet,
    builders::{
        ClientCapabilitiesBuilder, ClientCapabilitiesBuilderState, ServerCapabilitiesBuilder,
        ServerCapabilitiesBuilderState,
    },
};

pub use versioning::{VersionCompatibility, VersionManager, VersionRequirement};

/// Alias for RequestContext for backward compatibility
pub type Context = RequestContext;

/// Current MCP protocol version supported by this SDK
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// Supported protocol versions for compatibility
pub const SUPPORTED_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// Maximum message size in bytes (1MB) - Reduced for security (DoS protection)
pub const MAX_MESSAGE_SIZE: usize = 1024 * 1024;

/// Default timeout for operations in milliseconds
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// SDK version information
pub const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");

/// SDK name identifier
pub const SDK_NAME: &str = "turbomcp";

/// Protocol feature flags
pub mod features {
    /// Tool calling capability
    pub const TOOLS: &str = "tools";

    /// Prompt capability
    pub const PROMPTS: &str = "prompts";

    /// Resource capability
    pub const RESOURCES: &str = "resources";

    /// Logging capability
    pub const LOGGING: &str = "logging";

    /// Progress notifications
    pub const PROGRESS: &str = "progress";

    /// Sampling capability
    pub const SAMPLING: &str = "sampling";

    /// Roots capability
    pub const ROOTS: &str = "roots";
}

/// Protocol method names
pub mod methods {
    // Initialization
    /// Initialize handshake method
    pub const INITIALIZE: &str = "initialize";
    /// Initialized notification method
    pub const INITIALIZED: &str = "notifications/initialized";

    // Tools
    /// List available tools method
    pub const LIST_TOOLS: &str = "tools/list";
    /// Call a specific tool method
    pub const CALL_TOOL: &str = "tools/call";

    // Prompts
    /// List available prompts method
    pub const LIST_PROMPTS: &str = "prompts/list";
    /// Get a specific prompt method
    pub const GET_PROMPT: &str = "prompts/get";

    // Resources
    /// List available resources method
    pub const LIST_RESOURCES: &str = "resources/list";
    /// Read a specific resource method
    pub const READ_RESOURCE: &str = "resources/read";
    /// Subscribe to resource updates method
    pub const SUBSCRIBE: &str = "resources/subscribe";
    /// Unsubscribe from resource updates method
    pub const UNSUBSCRIBE: &str = "resources/unsubscribe";
    /// Resource updated notification
    pub const RESOURCE_UPDATED: &str = "notifications/resources/updated";
    /// Resource list changed notification
    pub const RESOURCE_LIST_CHANGED: &str = "notifications/resources/list_changed";

    // Logging
    /// Set logging level method
    pub const SET_LEVEL: &str = "logging/setLevel";
    /// Log message notification
    pub const LOG_MESSAGE: &str = "notifications/message";

    // Progress
    /// Progress update notification
    pub const PROGRESS: &str = "notifications/progress";

    // Sampling
    /// Create sampling message method
    pub const CREATE_MESSAGE: &str = "sampling/createMessage";

    // Roots
    /// List directory roots method
    pub const LIST_ROOTS: &str = "roots/list";
    /// Roots list changed notification
    pub const ROOTS_LIST_CHANGED: &str = "notifications/roots/list_changed";
}

/// Protocol error codes (JSON-RPC standard + MCP extensions)
pub mod error_codes {
    // JSON-RPC standard errors
    /// Parse error - Invalid JSON was received by the server
    pub const PARSE_ERROR: i32 = -32700;
    /// Invalid request - The JSON sent is not a valid Request object
    pub const INVALID_REQUEST: i32 = -32600;
    /// Method not found - The method does not exist / is not available
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// Invalid params - Invalid method parameter(s)
    pub const INVALID_PARAMS: i32 = -32602;
    /// Internal error - Internal JSON-RPC error
    pub const INTERNAL_ERROR: i32 = -32603;

    // MCP-specific errors (application-defined range)
    /// Tool not found error
    pub const TOOL_NOT_FOUND: i32 = -32001;
    /// Tool execution error
    pub const TOOL_EXECUTION_ERROR: i32 = -32002;
    /// Prompt not found error
    pub const PROMPT_NOT_FOUND: i32 = -32003;
    /// Resource not found error
    pub const RESOURCE_NOT_FOUND: i32 = -32004;
    /// Resource access denied error
    pub const RESOURCE_ACCESS_DENIED: i32 = -32005;
    /// Capability not supported error
    pub const CAPABILITY_NOT_SUPPORTED: i32 = -32006;
    /// Protocol version mismatch error
    pub const PROTOCOL_VERSION_MISMATCH: i32 = -32007;
    /// Authentication required error
    pub const AUTHENTICATION_REQUIRED: i32 = -32008;
    /// Rate limited error
    pub const RATE_LIMITED: i32 = -32009;
    /// Server overloaded error
    pub const SERVER_OVERLOADED: i32 = -32010;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_constants() {
        assert_eq!(PROTOCOL_VERSION, "2025-06-18");
        assert!(SUPPORTED_VERSIONS.contains(&PROTOCOL_VERSION));
    }

    #[test]
    fn test_size_constants() {
        // Constants are statically verified at compile-time
        const _: () = assert!(
            MAX_MESSAGE_SIZE > 1024,
            "MAX_MESSAGE_SIZE must be larger than 1KB"
        );
        const _: () = assert!(
            MAX_MESSAGE_SIZE == 1024 * 1024,
            "MAX_MESSAGE_SIZE must be 1MB for security"
        );

        const _: () = assert!(
            DEFAULT_TIMEOUT_MS > 1000,
            "DEFAULT_TIMEOUT_MS must be larger than 1 second"
        );
        const _: () = assert!(
            DEFAULT_TIMEOUT_MS == 30_000,
            "DEFAULT_TIMEOUT_MS must be 30 seconds"
        );
    }

    #[test]
    fn test_method_names() {
        assert_eq!(methods::INITIALIZE, "initialize");
        assert_eq!(methods::LIST_TOOLS, "tools/list");
        assert_eq!(methods::CALL_TOOL, "tools/call");
    }

    #[test]
    fn test_error_codes() {
        assert_eq!(error_codes::PARSE_ERROR, -32700);
        assert_eq!(error_codes::TOOL_NOT_FOUND, -32001);
    }
}
