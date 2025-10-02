//! MCP Protocol Types Module
//!
//! This module contains all the type definitions for the Model Context Protocol
//! organized into focused submodules based on the MCP 2025-06-18 specification.
//!
//! # Module Organization
//!
//! - [`core`] - Core protocol types and utilities
//! - [`capabilities`] - Client/server capability negotiation
//! - [`content`] - Message content types (text, image, audio, resources)
//! - [`requests`] - Request/response/notification enums
//! - [`initialization`] - Connection handshake types
//! - [`tools`] - Tool calling and execution
//! - [`prompts`] - Prompt templates
//! - [`resources`] - Resource access and templates
//! - [`logging`] - Logging and progress tracking
//! - [`sampling`] - LLM sampling (MCP 2025-06-18)
//! - [`elicitation`] - User input elicitation (MCP 2025-06-18)
//! - [`roots`] - Filesystem boundaries (MCP 2025-06-18)
//! - [`completion`] - Argument autocompletion
//! - [`ping`] - Connection testing

pub mod capabilities;
pub mod completion;
pub mod content;
pub mod core;
pub mod elicitation;
pub mod initialization;
pub mod logging;
pub mod ping;
pub mod prompts;
pub mod requests;
pub mod resources;
pub mod roots;
pub mod sampling;
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
pub use sampling::*;
pub use tools::*;
