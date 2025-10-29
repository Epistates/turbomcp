//! Introspection layer for discovering MCP server capabilities
//!
//! This module provides the ability to connect to any MCP server and discover
//! its capabilities via the MCP protocol, leveraging turbomcp-protocol and
//! turbomcp-transport for maximum correctness.

pub mod backends;
pub mod introspector;
pub mod spec;

// Re-export core types
pub use backends::{stdio::StdioBackend, McpBackend};
pub use introspector::McpIntrospector;
pub use spec::*;
