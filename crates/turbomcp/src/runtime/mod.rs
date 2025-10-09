//! Runtime support for full MCP 2025-06-18 protocol
//!
//! This module provides runtime dispatchers and helpers for implementing
//! the complete MCP protocol (including server→client requests like sampling,
//! elicitation, roots, ping) across different transport layers.

pub mod stdio_bidirectional;

#[cfg(feature = "http")]
pub mod http_bidirectional;

#[cfg(feature = "websocket")]
pub mod websocket_bidirectional;

#[cfg(all(feature = "websocket", feature = "http"))]
pub mod websocket_server;
