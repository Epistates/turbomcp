//! MCP Client for WASI environments
//!
//! This module provides a full MCP client implementation that works with
//! WASI Preview 2 transports (STDIO and HTTP).

use super::http::HttpTransport;
use super::stdio::StdioTransport;
use super::transport::{Transport, TransportError};
use serde::{Deserialize, Serialize};
use turbomcp_core::types::{
    capabilities::{ClientCapabilities, ServerCapabilities},
    content::ResourceContent,
    core::Implementation,
    initialization::InitializeResult,
    prompts::{GetPromptResult, Prompt},
    resources::{Resource, ResourceTemplate},
    tools::{CallToolResult, Tool},
};

/// Transport type for the MCP client
enum TransportKind {
    /// STDIO transport for direct process communication
    Stdio(StdioTransport),
    /// HTTP transport for HTTP-based MCP servers
    Http(HttpTransport),
}

impl TransportKind {
    fn request<P, R>(&self, method: &str, params: Option<P>) -> Result<R, TransportError>
    where
        P: Serialize,
        R: serde::de::DeserializeOwned,
    {
        match self {
            Self::Stdio(t) => t.request(method, params),
            Self::Http(t) => t.request(method, params),
        }
    }

    fn notify<P>(&self, method: &str, params: Option<P>) -> Result<(), TransportError>
    where
        P: Serialize,
    {
        match self {
            Self::Stdio(t) => t.notify(method, params),
            Self::Http(t) => t.notify(method, params),
        }
    }

    fn is_ready(&self) -> bool {
        match self {
            Self::Stdio(t) => t.is_ready(),
            Self::Http(t) => t.is_ready(),
        }
    }

    fn close(&self) -> Result<(), TransportError> {
        match self {
            Self::Stdio(t) => t.close(),
            Self::Http(t) => t.close(),
        }
    }
}

/// MCP Client for WASI environments
///
/// Provides full MCP protocol support using WASI Preview 2 interfaces.
/// Supports both STDIO and HTTP transports.
///
/// # Example with STDIO
///
/// ```ignore
/// use turbomcp_wasm::wasi::{McpClient, StdioTransport};
///
/// let transport = StdioTransport::new();
/// let mut client = McpClient::with_stdio(transport);
///
/// client.initialize()?;
/// let tools = client.list_tools()?;
/// for tool in &tools {
///     println!("Tool: {}", tool.name);
/// }
/// ```
///
/// # Example with HTTP
///
/// ```ignore
/// use turbomcp_wasm::wasi::{McpClient, HttpTransport};
///
/// let transport = HttpTransport::new("https://api.example.com/mcp")
///     .with_header("Authorization", "Bearer token");
/// let mut client = McpClient::with_http(transport);
///
/// client.initialize()?;
/// let result = client.call_tool("my_tool", serde_json::json!({"arg": "value"}))?;
/// ```
pub struct McpClient {
    /// Transport for communication
    transport: TransportKind,
    /// Whether the client has been initialized
    initialized: bool,
    /// Server information (after initialization)
    server_info: Option<Implementation>,
    /// Server capabilities (after initialization)
    server_capabilities: Option<ServerCapabilities>,
    /// Negotiated protocol version
    protocol_version: String,
}

impl McpClient {
    /// Create a new MCP client with STDIO transport
    #[must_use]
    pub fn with_stdio(transport: StdioTransport) -> Self {
        Self {
            transport: TransportKind::Stdio(transport),
            initialized: false,
            server_info: None,
            server_capabilities: None,
            protocol_version: "2025-11-25".to_string(),
        }
    }

    /// Create a new MCP client with HTTP transport
    #[must_use]
    pub fn with_http(transport: HttpTransport) -> Self {
        Self {
            transport: TransportKind::Http(transport),
            initialized: false,
            server_info: None,
            server_capabilities: None,
            protocol_version: "2025-11-25".to_string(),
        }
    }

    /// Initialize the MCP session
    ///
    /// This must be called before any other operations.
    pub fn initialize(&mut self) -> Result<InitializeResult, TransportError> {
        let params = InitializeParams {
            protocol_version: self.protocol_version.clone(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "turbomcp-wasm".to_string(),
                title: Some("TurboMCP WASI Client".to_string()),
                description: Some("MCP client running in WASI Preview 2 environment".to_string()),
                version: env!("CARGO_PKG_VERSION").to_string(),
                icon: None,
            },
        };

        let result: InitializeResult = self.transport.request("initialize", Some(params))?;

        self.initialized = true;
        self.server_info = Some(result.server_info.clone());
        self.server_capabilities = Some(result.capabilities.clone());
        self.protocol_version = result.protocol_version.clone();

        // Send initialized notification
        self.transport
            .notify("notifications/initialized", None::<()>)?;

        Ok(result)
    }

    /// Check if the client has been initialized
    #[must_use]
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Get server information (after initialization)
    #[must_use]
    pub fn server_info(&self) -> Option<&Implementation> {
        self.server_info.as_ref()
    }

    /// Get server capabilities (after initialization)
    #[must_use]
    pub fn server_capabilities(&self) -> Option<&ServerCapabilities> {
        self.server_capabilities.as_ref()
    }

    /// Get the negotiated protocol version
    #[must_use]
    pub fn protocol_version(&self) -> &str {
        &self.protocol_version
    }

    /// List available tools
    pub fn list_tools(&self) -> Result<Vec<Tool>, TransportError> {
        self.ensure_initialized()?;
        let result: ListToolsResult = self.transport.request("tools/list", None::<()>)?;
        Ok(result.tools)
    }

    /// Call a tool
    pub fn call_tool(
        &self,
        name: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResult, TransportError> {
        self.ensure_initialized()?;

        let params = CallToolParams {
            name: name.to_string(),
            arguments,
        };

        self.transport.request("tools/call", Some(params))
    }

    /// List available resources
    pub fn list_resources(&self) -> Result<Vec<Resource>, TransportError> {
        self.ensure_initialized()?;
        let result: ListResourcesResult = self.transport.request("resources/list", None::<()>)?;
        Ok(result.resources)
    }

    /// Read a resource
    pub fn read_resource(&self, uri: &str) -> Result<Vec<ResourceContent>, TransportError> {
        self.ensure_initialized()?;

        let params = ReadResourceParams {
            uri: uri.to_string(),
        };

        let result: ReadResourceResult = self.transport.request("resources/read", Some(params))?;
        Ok(result.contents)
    }

    /// List resource templates
    pub fn list_resource_templates(&self) -> Result<Vec<ResourceTemplate>, TransportError> {
        self.ensure_initialized()?;
        let result: ListResourceTemplatesResult = self
            .transport
            .request("resources/templates/list", None::<()>)?;
        Ok(result.resource_templates)
    }

    /// List available prompts
    pub fn list_prompts(&self) -> Result<Vec<Prompt>, TransportError> {
        self.ensure_initialized()?;
        let result: ListPromptsResult = self.transport.request("prompts/list", None::<()>)?;
        Ok(result.prompts)
    }

    /// Get a prompt
    pub fn get_prompt(
        &self,
        name: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<GetPromptResult, TransportError> {
        self.ensure_initialized()?;

        let params = GetPromptParams {
            name: name.to_string(),
            arguments,
        };

        self.transport.request("prompts/get", Some(params))
    }

    /// Ping the server
    pub fn ping(&self) -> Result<(), TransportError> {
        let _: serde_json::Value = self.transport.request("ping", None::<()>)?;
        Ok(())
    }

    /// Close the client connection
    pub fn close(&self) -> Result<(), TransportError> {
        self.transport.close()
    }

    /// Check if the transport is ready
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.transport.is_ready()
    }

    // Private helpers

    fn ensure_initialized(&self) -> Result<(), TransportError> {
        if !self.initialized {
            Err(TransportError::Protocol(
                "Client not initialized. Call initialize() first.".to_string(),
            ))
        } else {
            Ok(())
        }
    }
}

// Request/Response types

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InitializeParams {
    protocol_version: String,
    capabilities: ClientCapabilities,
    client_info: Implementation,
}

#[derive(Serialize)]
struct CallToolParams {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct ReadResourceParams {
    uri: String,
}

#[derive(Serialize)]
struct GetPromptParams {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct ListToolsResult {
    tools: Vec<Tool>,
}

#[derive(Deserialize)]
struct ListResourcesResult {
    resources: Vec<Resource>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListResourceTemplatesResult {
    resource_templates: Vec<ResourceTemplate>,
}

#[derive(Deserialize)]
struct ReadResourceResult {
    contents: Vec<ResourceContent>,
}

#[derive(Deserialize)]
struct ListPromptsResult {
    prompts: Vec<Prompt>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_with_stdio() {
        let transport = StdioTransport::new();
        let client = McpClient::with_stdio(transport);
        assert!(!client.is_initialized());
        assert!(client.is_ready());
    }

    #[test]
    fn test_client_with_http() {
        let transport = HttpTransport::new("https://api.example.com/mcp");
        let client = McpClient::with_http(transport);
        assert!(!client.is_initialized());
        assert!(client.is_ready());
    }

    #[test]
    fn test_client_protocol_version() {
        let transport = StdioTransport::new();
        let client = McpClient::with_stdio(transport);
        assert_eq!(client.protocol_version(), "2025-11-25");
    }

    #[test]
    fn test_ensure_initialized_fails() {
        let transport = StdioTransport::new();
        let client = McpClient::with_stdio(transport);
        let result = client.ensure_initialized();
        assert!(result.is_err());
    }
}
