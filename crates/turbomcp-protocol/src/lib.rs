//! # MCP Protocol Implementation
//!
//! This crate provides a complete implementation of the Model Context Protocol (MCP)
//! specification version 2025-06-18. It includes all protocol types, JSON-RPC integration,
//! and capability negotiation.
//!
//! ## Features
//!
//! - Complete MCP 2025-06-18 protocol implementation  
//! - JSON-RPC 2.0 support with batching
//! - Type-safe capability negotiation
//! - Protocol versioning and compatibility
//! - Fast serialization
//! - Comprehensive validation

#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub,
    clippy::all
)]
#![deny(unsafe_code)]
#![allow(
    clippy::module_name_repetitions,
    clippy::missing_errors_doc,  // Error documentation in progress
    clippy::wildcard_imports,  // Used in test modules
    clippy::must_use_candidate,  // Too pedantic for library APIs
    clippy::return_self_not_must_use,  // Constructor methods don't need must_use
    clippy::struct_excessive_bools,  // Sometimes bools are the right design
    clippy::missing_panics_doc,  // Panic docs added where genuinely needed
    clippy::default_trait_access  // Default::default() is sometimes clearer
)]
#![cfg_attr(docsrs, feature(doc_cfg))]

// Re-export core functionality
pub use turbomcp_core::{Error, Result};

// Core protocol modules
pub mod capabilities;
pub mod jsonrpc;
pub mod types;
pub mod validation;
pub mod versioning;

// Re-export commonly used types
pub use types::{
    CallToolRequest,
    CallToolResult,
    // Capability types
    ClientCapabilities,
    ClientNotification,
    // Core types
    ClientRequest,
    // Content types
    Content,
    // Sampling
    CreateMessageRequest,
    CreateMessageResult,
    EmbeddedResource,

    GetPromptRequest,
    GetPromptResult,
    ImageContent,
    Implementation,

    InitializeRequest,
    InitializeResult,
    InitializedNotification,

    ListPromptsRequest,
    ListPromptsResult,

    ListResourcesRequest,
    ListResourcesResult,
    ListRootsRequest,
    ListRootsResult,
    ListToolsRequest,
    ListToolsResult,

    // Logging and progress
    LogLevel,
    LoggingNotification,
    ProgressNotification,
    ProgressToken,
    // Prompt types
    Prompt,
    PromptInput,
    ProtocolVersion,
    ReadResourceRequest,
    ReadResourceResult,
    RequestId,
    // Resource types
    Resource,
    ResourceContents,
    ResourceUpdatedNotification,

    // Roots
    Root,
    RootsListChangedNotification,
    SamplingMessage,

    ServerCapabilities,
    ServerNotification,
    ServerRequest,
    SetLevelRequest,
    SetLevelResult,

    SubscribeRequest,
    TextContent,
    // Tool types
    Tool,
    ToolInputSchema,
    ToolOutputSchema,
    UnsubscribeRequest,
};

pub use jsonrpc::{
    JsonRpcBatch, JsonRpcError, JsonRpcErrorCode, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, JsonRpcVersion,
};

pub use capabilities::{CapabilityMatcher, CapabilityNegotiator, CapabilitySet};

pub use versioning::{VersionCompatibility, VersionManager, VersionRequirement};

/// Current MCP protocol version
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// Supported MCP protocol versions
pub const SUPPORTED_VERSIONS: &[&str] = &["2025-06-18", "2024-11-05"];

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
    fn test_protocol_constants() {
        assert_eq!(PROTOCOL_VERSION, "2025-06-18");
        assert!(SUPPORTED_VERSIONS.contains(&PROTOCOL_VERSION));
        #[allow(clippy::const_is_empty)]
        {
            assert!(!SUPPORTED_VERSIONS.is_empty());
        }
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
