//! MCP Protocol Types Module
//!
//! This module contains all the type definitions for the Model Context Protocol
//! organized into focused submodules based on the MCP 2025-11-25 specification.
//!
//! # Module Organization
//!
//! - [`crate::types::core`] - Core protocol types and utilities
//! - [`crate::types::domain`] - Validated domain types (Uri, MimeType, Base64String)
//! - [`crate::types::capabilities`] - Client/server capability negotiation
//! - [`crate::types::content`] - Message content types (text, image, audio, resources)
//! - [`crate::types::requests`] - Request/response/notification enums
//! - [`crate::types::initialization`] - Connection handshake types
//! - [`crate::types::tools`] - Tool calling and execution
//! - [`crate::types::prompts`] - Prompt templates
//! - [`crate::types::resources`] - Resource access and templates
//! - [`crate::types::logging`] - Logging and progress tracking
//! - [`crate::types::sampling`] - LLM sampling (MCP 2025-11-25)
//! - [`crate::types::elicitation`] - User input elicitation (MCP 2025-11-25)
//! - [`crate::types::roots`] - Filesystem boundaries (MCP 2025-11-25)
//! - [`crate::types::completion`] - Argument autocompletion
//! - [`crate::types::ping`] - Connection testing
//! - [`crate::types::tasks`] - Tasks API for durable operations (MCP 2025-11-25)

pub mod capabilities;
pub mod completion;
pub mod content;
pub mod core;
pub mod domain;
pub mod elicitation;
pub mod initialization;
pub mod logging;
pub mod ping;
pub mod prompts;
pub mod requests;
pub mod resources;
pub mod roots;
pub mod sampling;
#[cfg(feature = "mcp-tasks")]
pub mod tasks;
pub mod tools;

// Re-export all types for backward compatibility
pub use capabilities::*;
pub use completion::*;
pub use content::*;
pub use core::*;
pub use elicitation::*;
pub use initialization::*;
pub use logging::*;
pub use ping::*;
pub use prompts::*;
pub use requests::*;
pub use resources::*;
pub use roots::*;
pub use sampling::{ModelHint, *};
#[cfg(feature = "mcp-tasks")]
pub use tasks::*;
pub use tools::*;

// Re-export validated domain types (these have the same names as type aliases in core,
// but are distinct types with validation. Core type aliases are preferred for backward compat)
pub use domain::{
    Base64Error,
    MimeTypeError,
    UriError,
    // Note: Uri, MimeType, and Base64String from domain are NOT glob re-exported
    // to avoid ambiguity with the type aliases in core. Access them via domain::Uri, etc.
};
