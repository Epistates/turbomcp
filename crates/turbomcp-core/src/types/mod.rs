//! MCP Protocol Types - no_std compatible.
//!
//! This module provides core MCP type definitions that can be used in `no_std` environments.
//!
//! ## Organization
//!
//! - [`core`]: Core types (Role, Implementation, Annotations)
//! - [`tools`]: Tool definitions and schemas
//! - [`resources`]: Resource types and templates
//! - [`prompts`]: Prompt definitions
//! - [`content`]: Message content types
//! - [`capabilities`]: Client/server capabilities
//! - [`initialization`]: Handshake types

pub mod capabilities;
pub mod content;
pub mod core;
pub mod initialization;
pub mod prompts;
pub mod resources;
pub mod tools;

// Re-export all types
pub use capabilities::*;
pub use content::*;
pub use core::*;
pub use initialization::*;
pub use prompts::*;
pub use resources::*;
pub use tools::*;
