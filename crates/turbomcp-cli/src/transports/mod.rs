//! Transport layer implementations for MCP server communication
//!
//! Provides three transport protocols:
//! - **HTTP**: JSON-RPC over HTTP with bearer authentication
//! - **WebSocket**: Real-time bidirectional communication
//! - **STDIO**: Process-based communication for local servers

mod common;

pub mod http;
pub mod stdio;
pub mod ws;
