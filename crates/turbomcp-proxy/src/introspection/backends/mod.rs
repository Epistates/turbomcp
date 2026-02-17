//! MCP Backend Implementations
//!
//! This module provides different backend implementations for connecting
//! to MCP servers over various transports.

use std::future::Future;
use std::pin::Pin;

use serde_json::Value;
use turbomcp_protocol::{InitializeRequest, InitializeResult};

use crate::error::ProxyResult;

pub mod stdio;
// NOTE: Phase 2 - HTTP backend for introspection
// pub mod http;

/// Trait for connecting to MCP servers via different transports
///
/// This trait abstracts over the transport layer, allowing the introspector
/// to work with STDIO, HTTP/SSE, WebSocket, and other transports uniformly.
pub trait McpBackend: Send + Sync {
    /// Initialize the connection and perform MCP handshake
    ///
    /// This sends the initialize request and returns the server's response,
    /// which contains server info and capabilities.
    fn initialize(
        &mut self,
        request: InitializeRequest,
    ) -> Pin<Box<dyn Future<Output = ProxyResult<InitializeResult>> + Send + '_>>;

    /// Call an MCP method with parameters
    ///
    /// # Arguments
    /// * `method` - The MCP method name (e.g., "tools/list", "resources/list")
    /// * `params` - JSON parameters for the method
    ///
    /// # Returns
    /// The JSON result from the server
    fn call_method<'a>(
        &'a mut self,
        method: &'a str,
        params: Value,
    ) -> Pin<Box<dyn Future<Output = ProxyResult<Value>> + Send + 'a>>;

    /// Send a notification (one-way message, no response expected)
    fn send_notification<'a>(
        &'a mut self,
        method: &'a str,
        params: Value,
    ) -> Pin<Box<dyn Future<Output = ProxyResult<()>> + Send + 'a>>;

    /// Gracefully shutdown the connection
    fn shutdown(&mut self) -> Pin<Box<dyn Future<Output = ProxyResult<()>> + Send + '_>>;

    /// Get a human-readable description of this backend
    fn description(&self) -> String;
}
