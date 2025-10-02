//! Client configuration types and utilities
//!
//! This module contains configuration structures for MCP client connections
//! and initialization results.

use turbomcp_protocol::types::ServerCapabilities;

/// Result of client initialization containing server information
#[derive(Debug, Clone)]
pub struct InitializeResult {
    /// Information about the server
    pub server_info: turbomcp_protocol::Implementation,

    /// Capabilities supported by the server
    pub server_capabilities: ServerCapabilities,
}

/// Connection configuration for the client
#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    /// Request timeout in milliseconds
    pub timeout_ms: u64,

    /// Maximum number of retry attempts
    pub max_retries: u32,

    /// Retry delay in milliseconds
    pub retry_delay_ms: u64,

    /// Keep-alive interval in milliseconds
    pub keepalive_ms: u64,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 30_000,    // 30 seconds
            max_retries: 3,        // 3 attempts
            retry_delay_ms: 1_000, // 1 second
            keepalive_ms: 60_000,  // 60 seconds
        }
    }
}
