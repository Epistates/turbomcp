//! MCP service trait definition
//!
//! This module defines the core MCP service trait that implementations
//! must provide to handle MCP protocol requests.

use crate::tower::SessionInfo;
use turbomcp_protocol::Result as McpResult;

/// Core MCP service trait
///
/// Implementations of this trait provide the business logic for handling
/// MCP protocol requests. The trait is designed to be object-safe to
/// allow for dynamic dispatch.
#[async_trait::async_trait]
pub trait McpService: Send + Sync + 'static {
    /// Process an MCP request and return a response
    ///
    /// # Arguments
    ///
    /// * `request` - The JSON-RPC request payload
    /// * `session` - Session information for the current request
    ///
    /// # Returns
    ///
    /// Returns the JSON response or an error if processing fails.
    async fn process_request(
        &self,
        request: serde_json::Value,
        session: &SessionInfo,
    ) -> McpResult<serde_json::Value>;

    /// Get service capabilities
    ///
    /// Returns the capabilities that this service supports,
    /// following the MCP protocol specification.
    fn get_capabilities(&self) -> serde_json::Value {
        serde_json::json!({
            "protocol_version": "2025-06-18",
            "capabilities": {
                "tools": true,
                "resources": true,
                "prompts": true,
                "logging": true
            }
        })
    }
}

impl std::fmt::Debug for dyn McpService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpService")
            .field("capabilities", &self.get_capabilities())
            .finish()
    }
}
