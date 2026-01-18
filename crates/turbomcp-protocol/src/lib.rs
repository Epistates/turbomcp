//! # TurboMCP Protocol
//!
//! Complete Model Context Protocol (MCP) implementation in Rust, providing all protocol types,
//! traits, context management, and message handling for building MCP applications.
//!
//! ## MCP Version Support
//!
//! TurboMCP v3.0 fully implements MCP 2025-11-25 with all specification features enabled
//! by default. No feature flags needed for core protocol capabilities.
//!
//! | Specification | Status | Notes |
//! |---------------|--------|-------|
//! | **MCP 2025-11-25** | ✅ Full Support | Icons, URL elicitation, sampling tools, enum improvements |
//! | **MCP 2025-06-18** | ✅ Compatible | Negotiated at runtime via protocol version |
//!
//! **Quick Start:**
//! ```toml
//! turbomcp-protocol = "3.0"
//! ```
//!
//! Only the experimental Tasks API (SEP-1686) requires a feature flag:
//! ```toml
//! turbomcp-protocol = { version = "3.0", features = ["experimental-tasks"] }
//! ```
//!
//! ## What's Inside
//!
//! This crate provides everything needed for MCP:
//!
//! - **Types**: All MCP request/response types (2025-06-18 stable + 2025-11-25 draft)
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
//! ### Core Protocol Support (MCP 2025-06-18 Stable)
//! - Complete MCP 2025-06-18 protocol implementation
//! - JSON-RPC 2.0 support with batching and notifications
//! - Type-safe capability negotiation and compatibility checking
//! - Protocol versioning with backward compatibility
//! - Fast serialization with SIMD acceleration
//!
//! ### Advanced Protocol Features (MCP 2025-06-18 Stable)
//! - **Elicitation Protocol** - Server-initiated user input requests with rich schema validation
//! - **Sampling Support** - Bidirectional LLM sampling with fully-typed interfaces
//! - **Roots Protocol** - Filesystem boundaries with `roots/list` support
//! - **Server-to-Client Requests** - Fully typed trait for sampling, elicitation, and roots
//! - **Comprehensive Schema Builders** - Type-safe builders for all schema types
//!
//! ### MCP 2025-11-25 Features (Always Enabled)
//!
//! All core MCP 2025-11-25 specification features are now always available:
//!
//! | Feature | SEP | Description |
//! |---------|-----|-------------|
//! | URL Elicitation | SEP-1036 | URL mode for OAuth/sensitive data collection |
//! | Sampling Tools | SEP-1577 | Tool calling in LLM sampling requests |
//! | Icons | SEP-973 | Icon metadata for tools, resources, prompts |
//! | Enum Improvements | SEP-1330 | Standards-based JSON Schema enum patterns |
//!
//! **Experimental Feature (requires feature flag):**
//! - `experimental-tasks` - Tasks API (SEP-1686) for long-running operations
//!
//! **Authentication & Security** (always enabled):
//! - SSRF protection for URL validation
//! - Client ID Metadata Documents (CIMD) for OAuth 2.1
//! - OpenID Connect Discovery (RFC 8414 + OIDC 1.0)
//! - Incremental consent with WWW-Authenticate (SEP-835)
//!
//! ### Performance & Observability
//! - **SIMD-Accelerated JSON** - Fast processing with `simd-json` and `sonic-rs`
//! - **Zero-Copy Processing** - Memory-efficient message handling with `Bytes`
//! - **Request Context** - Full request/response context tracking for observability
//! - **Session Management** - Memory-bounded state management with cleanup tasks
//! - **Observability Ready** - Built-in support for tracing and metrics collection
//!
//! ## Version Selection
//!
//! TurboMCP v3.0 includes all MCP 2025-11-25 features by default. Runtime protocol
//! version negotiation determines actual feature availability per session.
//!
//! **Typical Usage:**
//! ```toml
//! [dependencies]
//! turbomcp-protocol = "3.0"  # All core features included
//! ```
//!
//! **With Experimental Tasks API:**
//! ```toml
//! [dependencies]
//! turbomcp-protocol = { version = "3.0", features = ["experimental-tasks"] }
//! ```
//!
//! ### Runtime Version Negotiation
//!
//! Clients and servers negotiate protocol versions during initialization:
//!
//! ```rust,no_run
//! use turbomcp_protocol::{InitializeRequest, InitializeResult, ClientCapabilities};
//! use turbomcp_protocol::types::{Implementation, ServerCapabilities}; // Corrected import path
//!
//! // Client requests draft features
//! let request = InitializeRequest {
//!     protocol_version: "2025-11-25".into(),  // Request draft
//!     capabilities: ClientCapabilities::default(),
//!     client_info: Implementation {
//!         name: "my-client".to_string(),
//!         title: None,
//!         version: "1.0.0".to_string(),
//!         ..Default::default()
//!     },
//!     _meta: None,
//! };
//!
//! // Server responds with actual supported version
//! // (may downgrade to 2025-06-18 if draft features unavailable)
//! let response = InitializeResult {
//!     protocol_version: "2025-11-25".into(),
//!     capabilities: ServerCapabilities::default(),
//!     server_info: Implementation {
//!         name: "my-server".to_string(),
//!         title: None,
//!         version: "1.0.0".to_string(),
//!         ..Default::default()
//!     },
//!     instructions: None,
//!     _meta: None,
//! };
//! ```
//!
//! **Key Principle:** Clients request, servers decide. The negotiated version is the server's response.
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
//!             task: None,
//!             tools: None,
//!             tool_choice: None,
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

// v3.0: Re-export turbomcp-core foundation types
/// Re-export of turbomcp-core, the no_std foundation layer
pub use turbomcp_core as core;

// v3.0: McpError is THE error type - re-export at crate root
pub use turbomcp_core::error::{ErrorContext as McpErrorContext, ErrorKind, McpError, McpResult};
/// v3.0 Result alias using McpError
pub type Result<T> = McpResult<T>;
/// v3.0 Error alias for migration (prefer McpError directly)
pub type Error = McpError;

// v3.0: Unified handler response types from core
// These enable the IntoToolResponse pattern for ergonomic tool handlers
pub use turbomcp_core::response::{Image, IntoToolError, IntoToolResponse, Json, Text, ToolError};

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

/// Zero-copy rkyv bridge for internal message routing.
///
/// This module is only available when the `rkyv` feature is enabled.
/// It provides efficient conversion between JSON-RPC and rkyv internal formats.
#[cfg(feature = "rkyv")]
#[cfg_attr(docsrs, doc(cfg(feature = "rkyv")))]
pub mod rkyv_bridge;

/// Wire codec integration for message serialization.
///
/// This module provides a unified interface for encoding/decoding MCP messages
/// using the [`turbomcp_wire`] codec abstraction.
///
/// Enable with the `wire` feature flag. Optional SIMD acceleration available
/// with `wire-simd`, and MessagePack support with `wire-msgpack`.
#[cfg(feature = "wire")]
#[cfg_attr(docsrs, doc(cfg(feature = "wire")))]
pub mod codec;

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
// v3.0: McpError is re-exported from turbomcp_core at crate root
pub use error::RetryInfo;
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

pub use jsonrpc::{
    JsonRpcError, JsonRpcErrorCode, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse,
    JsonRpcVersion,
};

pub use capabilities::{
    CapabilityMatcher, CapabilityNegotiator, CapabilitySet,
    builders::{
        ClientCapabilitiesBuilder, ClientCapabilitiesBuilderState, ServerCapabilitiesBuilder,
        ServerCapabilitiesBuilderState,
    },
};

pub use versioning::{VersionCompatibility, VersionManager, VersionRequirement};

// Re-export constants from core (single source of truth - DRY)
pub use turbomcp_core::{
    DEFAULT_TIMEOUT_MS, MAX_MESSAGE_SIZE, PROTOCOL_VERSION, SDK_NAME, SDK_VERSION,
    SUPPORTED_VERSIONS, error_codes, features, methods,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_constants() {
        assert_eq!(PROTOCOL_VERSION, "2025-11-25");
        assert!(SUPPORTED_VERSIONS.contains(&PROTOCOL_VERSION));
        // Latest should be first in supported versions
        assert_eq!(SUPPORTED_VERSIONS[0], PROTOCOL_VERSION);
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
