//! MCP Backend Implementations
//!
//! This module provides different backend implementations for connecting
//! to MCP servers over various transports.

use async_trait::async_trait;
use serde_json::Value;
use turbomcp_protocol::{InitializeRequest, InitializeResult};

use crate::error::ProxyResult;

pub mod stdio;
// TODO: HTTP backend in Phase 1
// pub mod http;

/// Trait for connecting to MCP servers via different transports
///
/// This trait abstracts over the transport layer, allowing the introspector
/// to work with STDIO, HTTP/SSE, WebSocket, and other transports uniformly.
#[async_trait]
pub trait McpBackend: Send + Sync {
    /// Initialize the connection and perform MCP handshake
    ///
    /// This sends the initialize request and returns the server's response,
    /// which contains server info and capabilities.
    async fn initialize(&mut self, request: InitializeRequest) -> ProxyResult<InitializeResult>;

    /// Call an MCP method with parameters
    ///
    /// # Arguments
    /// * `method` - The MCP method name (e.g., "tools/list", "resources/list")
    /// * `params` - JSON parameters for the method
    ///
    /// # Returns
    /// The JSON result from the server
    async fn call_method(&mut self, method: &str, params: Value) -> ProxyResult<Value>;

    /// Send a notification (one-way message, no response expected)
    async fn send_notification(&mut self, method: &str, params: Value) -> ProxyResult<()>;

    /// Gracefully shutdown the connection
    async fn shutdown(&mut self) -> ProxyResult<()>;

    /// Get a human-readable description of this backend
    fn description(&self) -> String;
}
