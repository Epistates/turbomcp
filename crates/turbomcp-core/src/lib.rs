//! # TurboMCP Core
//!
//! Core MCP types and primitives - `no_std` compatible for WASM targets.
//!
//! This crate provides the foundational types for the Model Context Protocol (MCP)
//! that can be used in `no_std` environments including WebAssembly.
//!
//! ## Features
//!
//! - `std` (default): Enable standard library support, including richer error types
//! - `rich-errors`: Enable UUID-based error tracking (requires `std`)
//! - `wasm`: Enable WASM-specific optimizations
//! - `zero-copy`: Enable rkyv zero-copy serialization for internal message passing
//!
//! ## no_std Usage
//!
//! ```toml
//! [dependencies]
//! turbomcp-core = { version = "3.0", default-features = false }
//! ```
//!
//! ## Module Organization
//!
//! - [`types`]: Core MCP protocol types (tools, resources, prompts, etc.)
//! - [`error`]: Error types and handling
//! - [`jsonrpc`]: JSON-RPC 2.0 types
//!
//! ## Example
//!
//! ```rust
//! use turbomcp_core::types::{Tool, ToolInputSchema};
//! use turbomcp_core::error::{McpError, ErrorKind};
//!
//! // Create a tool definition
//! let tool = Tool {
//!     name: "calculator".into(),
//!     description: Some("Performs calculations".into()),
//!     input_schema: ToolInputSchema::default(),
//!     ..Default::default()
//! };
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

pub mod error;
pub mod jsonrpc;
pub mod types;

/// Zero-copy message types using rkyv serialization.
///
/// This module is only available when the `zero-copy` feature is enabled.
/// It provides internal message types optimized for zero-copy deserialization.
#[cfg(feature = "zero-copy")]
#[cfg_attr(docsrs, doc(cfg(feature = "zero-copy")))]
pub mod rkyv_types;

// Re-export commonly used types at crate root
pub use error::{ErrorKind, McpError, McpResult};
pub use jsonrpc::{
    JSONRPC_VERSION, JsonRpcError, JsonRpcErrorCode, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, JsonRpcVersion,
};

/// MCP Protocol version supported by this SDK (latest official spec)
pub const PROTOCOL_VERSION: &str = "2025-11-25";

/// Supported protocol versions in preference order (latest first)
pub const SUPPORTED_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// Maximum message size in bytes (1MB)
pub const MAX_MESSAGE_SIZE: usize = 1024 * 1024;

/// Default timeout for operations in milliseconds
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// SDK version
pub const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");

/// SDK name
pub const SDK_NAME: &str = "turbomcp";

/// Protocol feature constants
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
    /// Initialize handshake method
    pub const INITIALIZE: &str = "initialize";
    /// Initialized notification method
    pub const INITIALIZED: &str = "notifications/initialized";
    /// List available tools method
    pub const LIST_TOOLS: &str = "tools/list";
    /// Call a specific tool method
    pub const CALL_TOOL: &str = "tools/call";
    /// List available prompts method
    pub const LIST_PROMPTS: &str = "prompts/list";
    /// Get a specific prompt method
    pub const GET_PROMPT: &str = "prompts/get";
    /// List available resources method
    pub const LIST_RESOURCES: &str = "resources/list";
    /// Read a specific resource method
    pub const READ_RESOURCE: &str = "resources/read";
    /// Subscribe to resource updates method
    pub const SUBSCRIBE: &str = "resources/subscribe";
    /// Unsubscribe from resource updates method
    pub const UNSUBSCRIBE: &str = "resources/unsubscribe";
    /// Set logging level method
    pub const SET_LEVEL: &str = "logging/setLevel";
    /// Create sampling message method
    pub const CREATE_MESSAGE: &str = "sampling/createMessage";
    /// List directory roots method
    pub const LIST_ROOTS: &str = "roots/list";
}

/// Protocol error codes (JSON-RPC standard + MCP extensions)
pub mod error_codes {
    /// Parse error (-32700)
    pub const PARSE_ERROR: i32 = -32700;
    /// Invalid request (-32600)
    pub const INVALID_REQUEST: i32 = -32600;
    /// Method not found (-32601)
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// Invalid params (-32602)
    pub const INVALID_PARAMS: i32 = -32602;
    /// Internal error (-32603)
    pub const INTERNAL_ERROR: i32 = -32603;
    /// URL elicitation required (-32042)
    pub const URL_ELICITATION_REQUIRED: i32 = -32042;
    /// Tool not found (-32001)
    pub const TOOL_NOT_FOUND: i32 = -32001;
    /// Tool execution error (-32002)
    pub const TOOL_EXECUTION_ERROR: i32 = -32002;
    /// Prompt not found (-32003)
    pub const PROMPT_NOT_FOUND: i32 = -32003;
    /// Resource not found (-32004)
    pub const RESOURCE_NOT_FOUND: i32 = -32004;
    /// Resource access denied (-32005)
    pub const RESOURCE_ACCESS_DENIED: i32 = -32005;
    /// Capability not supported (-32006)
    pub const CAPABILITY_NOT_SUPPORTED: i32 = -32006;
    /// Protocol version mismatch (-32007)
    pub const PROTOCOL_VERSION_MISMATCH: i32 = -32007;
    /// Authentication required (-32008)
    pub const AUTHENTICATION_REQUIRED: i32 = -32008;
    /// Rate limited (-32009)
    pub const RATE_LIMITED: i32 = -32009;
    /// Server overloaded (-32010)
    pub const SERVER_OVERLOADED: i32 = -32010;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_constants() {
        assert_eq!(PROTOCOL_VERSION, "2025-11-25");
        assert!(SUPPORTED_VERSIONS.contains(&PROTOCOL_VERSION));
        assert_eq!(SUPPORTED_VERSIONS[0], PROTOCOL_VERSION);
    }

    #[test]
    fn test_size_constants() {
        assert_eq!(MAX_MESSAGE_SIZE, 1024 * 1024);
        assert_eq!(DEFAULT_TIMEOUT_MS, 30_000);
    }
}
