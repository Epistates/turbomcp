//! Prelude module for convenient imports
//!
//! This module re-exports commonly used types for WASM MCP server development.
//!
//! # Usage
//!
//! ```ignore
//! use turbomcp_wasm::prelude::*;
//! ```

// Re-export unified error types from turbomcp-core (v3 architecture)
pub use turbomcp_core::error::{ErrorKind, McpError, McpResult};

// Re-export unified handler trait from turbomcp-core
pub use turbomcp_core::handler::McpHandler;

// Re-export core types needed for implementing handlers
pub use turbomcp_core::types::{prompts::Prompt, resources::Resource, tools::Tool};

// Re-export wasm_server types when available
#[cfg(feature = "wasm-server")]
pub use crate::wasm_server::{
    Image, IntoToolResponse, Json, McpServer, McpServerBuilder, PromptResult, ResourceResult, Text,
    ToolError, ToolResult, WasmHandlerExt,
};

// Re-export proc macros when available
#[cfg(feature = "macros")]
pub use crate::{prompt, resource, server, tool};

// Re-export worker types for convenience (excluding Result to avoid conflicts with ToolResult)
#[cfg(feature = "wasm-server")]
pub use worker::{Context, Env, Request, Response};
