//! Integration prelude for LLM framework integrations (Langchain, etc.)
//!
//! This module provides a comprehensive re-export of types commonly needed when
//! integrating MCP clients with LLM frameworks. It includes everything from the
//! standard prelude plus additional abstractions that are particularly useful for
//! building agent integrations.
//!
//! # Usage
//!
//! For LLM framework integrations, use this instead of the standard prelude:
//!
//! ```rust,no_run
//! use turbomcp_client::integration::*;
//!
//! // Now available:
//! // - Client<T> and ClientBuilder for MCP connections
//! // - Transport trait for generic bounds in agent definitions
//! // - Tool, Resource, Prompt for protocol types
//! // - All handler types for server-initiated requests
//!
//! // Example: Generic function that works with any transport type
//! pub fn process_tool_with_client<T: Transport + 'static>(
//!     tool: Tool,
//!     client: Client<T>,
//! ) {
//!     // Integration code here
//!     println!("Tool name: {}", tool.name);
//! }
//! ```

// Re-export the complete standard prelude
pub use crate::prelude::*;

// Additional re-exports specifically useful for integrations
// (most are already in prelude, but we keep this explicit for documentation)

// Core client types
pub use crate::ClientBuilder;

// Transport trait - essential for generic bounds
pub use turbomcp_transport::Transport;

// Re-export commonly needed protocol types (already in prelude but documented here)
pub use turbomcp_protocol::types::{
    Content, CreateMessageRequest, CreateMessageResult, Prompt, Resource, Tool,
};
