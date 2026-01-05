//! Initialization types for MCP handshake.

use alloc::string::String;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::capabilities::{ClientCapabilities, ServerCapabilities};
use super::core::{Implementation, ProtocolVersion};

/// Initialize request from client to server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeRequest {
    /// Protocol version requested by client
    #[serde(rename = "protocolVersion")]
    pub protocol_version: ProtocolVersion,
    /// Client capabilities
    pub capabilities: ClientCapabilities,
    /// Client implementation info
    #[serde(rename = "clientInfo")]
    pub client_info: Implementation,
    /// Optional metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
}

impl Default for InitializeRequest {
    fn default() -> Self {
        Self {
            protocol_version: crate::PROTOCOL_VERSION.into(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation::default(),
            _meta: None,
        }
    }
}

impl InitializeRequest {
    /// Create a new initialize request
    #[must_use]
    pub fn new(client_info: Implementation) -> Self {
        Self {
            protocol_version: crate::PROTOCOL_VERSION.into(),
            capabilities: ClientCapabilities::default(),
            client_info,
            _meta: None,
        }
    }

    /// Set capabilities
    #[must_use]
    pub fn with_capabilities(mut self, capabilities: ClientCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Set protocol version
    #[must_use]
    pub fn with_protocol_version(mut self, version: impl Into<String>) -> Self {
        self.protocol_version = version.into();
        self
    }
}

/// Initialize response from server to client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    /// Protocol version used by server
    #[serde(rename = "protocolVersion")]
    pub protocol_version: ProtocolVersion,
    /// Server capabilities
    pub capabilities: ServerCapabilities,
    /// Server implementation info
    #[serde(rename = "serverInfo")]
    pub server_info: Implementation,
    /// Optional instructions for the client
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Optional metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
}

impl Default for InitializeResult {
    fn default() -> Self {
        Self {
            protocol_version: crate::PROTOCOL_VERSION.into(),
            capabilities: ServerCapabilities::default(),
            server_info: Implementation::default(),
            instructions: None,
            _meta: None,
        }
    }
}

impl InitializeResult {
    /// Create a new initialize result
    #[must_use]
    pub fn new(server_info: Implementation) -> Self {
        Self {
            protocol_version: crate::PROTOCOL_VERSION.into(),
            capabilities: ServerCapabilities::default(),
            server_info,
            instructions: None,
            _meta: None,
        }
    }

    /// Set capabilities
    #[must_use]
    pub fn with_capabilities(mut self, capabilities: ServerCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Set instructions
    #[must_use]
    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }
}

/// Initialized notification (client -> server)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InitializedNotification {
    /// Optional metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
}

/// Ping request
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PingRequest {
    /// Optional metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
}

/// Shutdown request
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShutdownRequest {
    /// Optional metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initialize_request() {
        let req = InitializeRequest::new(Implementation::new("test-client", "1.0.0"))
            .with_capabilities(ClientCapabilities::new().with_sampling());

        assert_eq!(req.client_info.name, "test-client");
        assert!(req.capabilities.sampling.is_some());
    }

    #[test]
    fn test_initialize_result() {
        let res = InitializeResult::new(Implementation::new("test-server", "1.0.0"))
            .with_capabilities(ServerCapabilities::new().with_tools(true));

        assert_eq!(res.server_info.name, "test-server");
        assert!(res.capabilities.tools.is_some());
    }
}
