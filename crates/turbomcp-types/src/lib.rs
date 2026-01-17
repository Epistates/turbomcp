//! # TurboMCP Types
//!
//! Core types for the TurboMCP SDK - the foundation of MCP server development.
//!
//! This crate provides all shared types used across the TurboMCP ecosystem:
//!
//! - **Content types**: `Content`, `TextContent`, `ImageContent`, etc.
//! - **Result types**: `ToolResult`, `ResourceResult`, `PromptResult`
//! - **Definition types**: `Tool`, `Resource`, `Prompt`, `ServerInfo`
//! - **Conversion traits**: `IntoToolResult`, `IntoResourceResult`, `IntoPromptResult`
//!
//! For error handling, use `turbomcp_core::error::{McpError, McpResult}`.
//!
//! ## Design Principles
//!
//! 1. **Single Source of Truth**: These types are the canonical definitions
//! 2. **Ergonomic by Default**: Common operations are one-liners
//! 3. **MCP 2025-11-25 Compliant**: Full spec support
//! 4. **no_std Compatible**: Works in WASM and embedded environments
//!
//! ## Quick Start
//!
//! ```rust
//! use turbomcp_types::*;
//!
//! // Create a tool result
//! let result = ToolResult::text("Hello, world!");
//!
//! // Create an error result
//! let error = ToolResult::error("Something went wrong");
//!
//! // Create a JSON result with structured content
//! let json_result = ToolResult::json(&serde_json::json!({"key": "value"})).unwrap();
//!
//! // Create a resource result
//! let resource = ResourceResult::text("file:///example.txt", "File contents here");
//!
//! // Create a prompt result
//! let prompt = PromptResult::user("Hello!")
//!     .add_assistant("How can I help?")
//!     .with_description("A greeting prompt");
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod content;
pub mod definitions;
pub mod results;
pub mod traits;

// Re-export everything at the crate root for convenience
pub use content::*;
pub use definitions::*;
pub use results::*;
pub use traits::*;

/// Version of the TurboMCP types crate
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// MCP protocol version this crate targets
pub const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
