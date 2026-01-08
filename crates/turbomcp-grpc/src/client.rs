//! gRPC client implementation for MCP
//!
//! This module provides a tonic-based gRPC client for connecting to MCP servers.

use crate::error::{GrpcError, GrpcResult, status_to_mcp_error};
use crate::proto::{self, mcp_service_client::McpServiceClient};
use std::time::Duration;
use tonic::transport::{Channel, Endpoint};
use tracing::{debug, info, instrument};
use turbomcp_core::types::{
    capabilities::{ClientCapabilities, ServerCapabilities},
    content::ResourceContent,
    core::Implementation,
    initialization::InitializeResult,
    prompts::{GetPromptResult, Prompt},
    resources::{Resource, ResourceTemplate},
    tools::{CallToolResult, Tool},
};

/// gRPC client for MCP servers
#[derive(Clone)]
pub struct McpGrpcClient {
    /// The gRPC client
    client: McpServiceClient<Channel>,
    /// Client implementation info
    client_info: Implementation,
    /// Server info after initialization
    server_info: Option<Implementation>,
    /// Server capabilities after initialization
    server_capabilities: Option<ServerCapabilities>,
    /// Protocol version
    protocol_version: String,
}

/// Configuration for the gRPC client
#[derive(Debug, Clone)]
pub struct McpGrpcClientConfig {
    /// Client name
    pub name: String,
    /// Client version
    pub version: String,
    /// Protocol version to request
    pub protocol_version: String,
    /// Client capabilities to advertise
    pub capabilities: ClientCapabilities,
    /// Connection timeout
    pub connect_timeout: Duration,
    /// Request timeout
    pub request_timeout: Duration,
    /// Enable TLS
    pub tls: bool,
}

impl Default for McpGrpcClientConfig {
    fn default() -> Self {
        Self {
            name: "turbomcp-grpc-client".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: "2025-11-25".to_string(),
            capabilities: ClientCapabilities::default(),
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(30),
            tls: false,
        }
    }
}

impl McpGrpcClient {
    /// Connect to a gRPC server with default configuration
    ///
    /// # Arguments
    ///
    /// * `addr` - The server address (e.g., `http://[::1]:50051`)
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails.
    pub async fn connect(addr: impl AsRef<str>) -> GrpcResult<Self> {
        Self::connect_with_config(addr, McpGrpcClientConfig::default()).await
    }

    /// Connect to a gRPC server with custom configuration
    ///
    /// # Arguments
    ///
    /// * `addr` - The server address
    /// * `config` - Client configuration
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails.
    pub async fn connect_with_config(
        addr: impl AsRef<str>,
        config: McpGrpcClientConfig,
    ) -> GrpcResult<Self> {
        let endpoint = Endpoint::from_shared(addr.as_ref().to_string())
            .map_err(|e| GrpcError::config(format!("Invalid endpoint: {e}")))?
            .connect_timeout(config.connect_timeout)
            .timeout(config.request_timeout);

        let channel = endpoint.connect().await?;

        info!(addr = %addr.as_ref(), "Connected to gRPC server");

        Ok(Self {
            client: McpServiceClient::new(channel),
            client_info: Implementation {
                name: config.name,
                title: None,
                description: None,
                version: config.version,
                icon: None,
            },
            server_info: None,
            server_capabilities: None,
            protocol_version: config.protocol_version,
        })
    }

    /// Initialize the MCP session
    ///
    /// # Errors
    ///
    /// Returns an error if initialization fails.
    #[instrument(skip(self))]
    pub async fn initialize(&mut self) -> GrpcResult<InitializeResult> {
        let request = proto::InitializeRequest {
            protocol_version: self.protocol_version.clone(),
            capabilities: Some(ClientCapabilities::default().into()),
            client_info: Some(self.client_info.clone().into()),
        };

        let response = self
            .client
            .initialize(request)
            .await
            .map_err(|s| GrpcError::Mcp(status_to_mcp_error(&s)))?;

        let result = response.into_inner();

        // Store server info
        if let Some(ref info) = result.server_info {
            self.server_info = Some(info.clone().into());
        }

        // Store server capabilities
        if let Some(ref caps) = result.capabilities {
            self.server_capabilities = Some(caps.clone().into());
        }

        info!(
            server = ?self.server_info,
            protocol = %result.protocol_version,
            "MCP session initialized"
        );

        result.try_into()
    }

    /// Ping the server
    ///
    /// # Errors
    ///
    /// Returns an error if the ping fails.
    #[instrument(skip(self))]
    pub async fn ping(&mut self) -> GrpcResult<()> {
        self.client
            .ping(proto::PingRequest {})
            .await
            .map_err(|s| GrpcError::Mcp(status_to_mcp_error(&s)))?;

        debug!("Ping successful");
        Ok(())
    }

    /// List available tools
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    #[instrument(skip(self))]
    pub async fn list_tools(&mut self) -> GrpcResult<Vec<Tool>> {
        let response = self
            .client
            .list_tools(proto::ListToolsRequest { cursor: None })
            .await
            .map_err(|s| GrpcError::Mcp(status_to_mcp_error(&s)))?;

        let result = response.into_inner();
        let tools: Result<Vec<_>, _> = result.tools.into_iter().map(TryInto::try_into).collect();

        debug!(
            count = tools.as_ref().map(Vec::len).unwrap_or(0),
            "Listed tools"
        );
        tools
    }

    /// Call a tool
    ///
    /// # Arguments
    ///
    /// * `name` - The tool name
    /// * `arguments` - Optional arguments as JSON
    ///
    /// # Errors
    ///
    /// Returns an error if the tool call fails.
    #[instrument(skip(self, name, arguments))]
    pub async fn call_tool(
        &mut self,
        name: impl AsRef<str>,
        arguments: Option<serde_json::Value>,
    ) -> GrpcResult<CallToolResult> {
        let arguments_bytes = arguments.map(|v| serde_json::to_vec(&v)).transpose()?;

        let response = self
            .client
            .call_tool(proto::CallToolRequest {
                name: name.as_ref().to_string(),
                arguments: arguments_bytes,
            })
            .await
            .map_err(|s| GrpcError::Mcp(status_to_mcp_error(&s)))?;

        debug!(tool = %name.as_ref(), "Tool called");
        response.into_inner().try_into()
    }

    /// List available resources
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    #[instrument(skip(self))]
    pub async fn list_resources(&mut self) -> GrpcResult<Vec<Resource>> {
        let response = self
            .client
            .list_resources(proto::ListResourcesRequest { cursor: None })
            .await
            .map_err(|s| GrpcError::Mcp(status_to_mcp_error(&s)))?;

        let result = response.into_inner();
        let resources: Vec<_> = result.resources.into_iter().map(Into::into).collect();

        debug!(count = resources.len(), "Listed resources");
        Ok(resources)
    }

    /// List resource templates
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    #[instrument(skip(self))]
    pub async fn list_resource_templates(&mut self) -> GrpcResult<Vec<ResourceTemplate>> {
        let response = self
            .client
            .list_resource_templates(proto::ListResourceTemplatesRequest { cursor: None })
            .await
            .map_err(|s| GrpcError::Mcp(status_to_mcp_error(&s)))?;

        let result = response.into_inner();
        let templates: Vec<_> = result
            .resource_templates
            .into_iter()
            .map(Into::into)
            .collect();

        debug!(count = templates.len(), "Listed resource templates");
        Ok(templates)
    }

    /// Read a resource
    ///
    /// # Arguments
    ///
    /// * `uri` - The resource URI
    ///
    /// # Errors
    ///
    /// Returns an error if the read fails.
    #[instrument(skip(self, uri))]
    pub async fn read_resource(
        &mut self,
        uri: impl AsRef<str>,
    ) -> GrpcResult<Vec<ResourceContent>> {
        let response = self
            .client
            .read_resource(proto::ReadResourceRequest {
                uri: uri.as_ref().to_string(),
            })
            .await
            .map_err(|s| GrpcError::Mcp(status_to_mcp_error(&s)))?;

        let result = response.into_inner();
        let contents: Vec<_> = result.contents.into_iter().map(Into::into).collect();

        debug!(uri = %uri.as_ref(), count = contents.len(), "Read resource");
        Ok(contents)
    }

    /// List available prompts
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    #[instrument(skip(self))]
    pub async fn list_prompts(&mut self) -> GrpcResult<Vec<Prompt>> {
        let response = self
            .client
            .list_prompts(proto::ListPromptsRequest { cursor: None })
            .await
            .map_err(|s| GrpcError::Mcp(status_to_mcp_error(&s)))?;

        let result = response.into_inner();
        let prompts: Vec<_> = result.prompts.into_iter().map(Into::into).collect();

        debug!(count = prompts.len(), "Listed prompts");
        Ok(prompts)
    }

    /// Get a prompt
    ///
    /// # Arguments
    ///
    /// * `name` - The prompt name
    /// * `arguments` - Optional arguments as JSON
    ///
    /// # Errors
    ///
    /// Returns an error if the get fails.
    #[instrument(skip(self, name, arguments))]
    pub async fn get_prompt(
        &mut self,
        name: impl AsRef<str>,
        arguments: Option<serde_json::Value>,
    ) -> GrpcResult<GetPromptResult> {
        let arguments_bytes = arguments.map(|v| serde_json::to_vec(&v)).transpose()?;

        let response = self
            .client
            .get_prompt(proto::GetPromptRequest {
                name: name.as_ref().to_string(),
                arguments: arguments_bytes,
            })
            .await
            .map_err(|s| GrpcError::Mcp(status_to_mcp_error(&s)))?;

        debug!(prompt = %name.as_ref(), "Got prompt");
        response.into_inner().try_into()
    }

    /// Get server info (after initialization)
    #[must_use]
    pub fn server_info(&self) -> Option<&Implementation> {
        self.server_info.as_ref()
    }

    /// Get server capabilities (after initialization)
    #[must_use]
    pub fn server_capabilities(&self) -> Option<&ServerCapabilities> {
        self.server_capabilities.as_ref()
    }

    /// Get the protocol version
    #[must_use]
    pub fn protocol_version(&self) -> &str {
        &self.protocol_version
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_config_default() {
        let config = McpGrpcClientConfig::default();
        assert_eq!(config.protocol_version, "2025-11-25");
        assert_eq!(config.connect_timeout, Duration::from_secs(10));
        assert!(!config.tls);
    }
}
