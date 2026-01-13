//! Prelude module for convenient imports
//!
//! This module re-exports commonly used types for WASM MCP server development.
//!
//! # Usage
//!
//! ```ignore
//! use turbomcp_wasm::prelude::*;
//! ```

// Re-export wasm_server types when available
#[cfg(feature = "wasm-server")]
pub use crate::wasm_server::{
    Image, IntoToolResponse, Json, McpServer, McpServerBuilder, PromptResult, ResourceResult, Text,
    ToolError, ToolResult,
};

// Re-export proc macros when available
#[cfg(feature = "macros")]
pub use crate::{prompt, resource, server, tool};

// Re-export worker types for convenience
#[cfg(feature = "wasm-server")]
pub use worker::{Context, Env, Request, Response, Result};
