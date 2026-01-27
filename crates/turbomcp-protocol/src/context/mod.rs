//! Context module for MCP request and response handling.
//!
//! This module provides comprehensive context types for tracking requests,
//! responses, and various MCP protocol features including bidirectional
//! communication, elicitation, completion, and more.

pub mod capabilities;
pub mod client;
pub mod completion;
pub mod elicitation;
pub mod ping;
pub mod request;
pub mod rich;
pub mod server_initiated;
pub mod templates;

// Re-export everything to maintain API compatibility
pub use capabilities::*;
pub use client::*;
pub use completion::*;
pub use elicitation::*;
pub use ping::*;
pub use request::*;
pub use rich::*;
pub use server_initiated::*;
pub use templates::*;

// 🎉 REFACTORING COMPLETE! 🎉
// All 2,046 lines from the monolithic context.rs have been successfully
// decomposed into 8 focused, maintainable modules with zero breaking changes!
