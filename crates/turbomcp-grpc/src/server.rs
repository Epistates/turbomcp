//! gRPC server implementation for MCP
//!
//! This module provides a tonic-based gRPC server that implements the MCP protocol.

// Type conversions are handled via From/Into traits
use crate::error::{GrpcError, GrpcResult};
use crate::proto::{
    self,
    mcp_service_server::{McpService, McpServiceServer},
};
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tokio_stream::Stream;
use tonic::{Request, Response, Status};
use tracing::{debug, info, instrument};
use turbomcp_core::types::{
    capabilities::ServerCapabilities,
    content::ResourceContent,
    core::Implementation,
    prompts::{GetPromptResult, Prompt},
    resources::{Resource, ResourceTemplate},
    tools::{CallToolResult, Tool},
};

/// Notification sender type
type NotificationTx = broadcast::Sender<proto::Notification>;

/// gRPC server for MCP
pub struct McpGrpcServer {
    /// Server implementation info
    server_info: Implementation,
    /// Server capabilities
    capabilities: ServerCapabilities,
    /// Protocol version
    protocol_version: String,
    /// Server instructions
    instructions: Option<String>,
    /// Tool handlers
    tools: Arc<RwLock<Vec<Tool>>>,
    /// Resource handlers
    resources: Arc<RwLock<Vec<Resource>>>,
    /// Resource templates
    resource_templates: Arc<RwLock<Vec<ResourceTemplate>>>,
    /// Prompts
    prompts: Arc<RwLock<Vec<Prompt>>>,
    /// Notification broadcaster
    notification_tx: NotificationTx,
    /// Tool call handler
    tool_handler: Arc<dyn ToolHandler + Send + Sync>,
    /// Resource read handler
    resource_handler: Arc<dyn ResourceHandler + Send + Sync>,
    /// Prompt handler
    prompt_handler: Arc<dyn PromptHandler + Send + Sync>,
}

/// Trait for handling tool calls
#[async_trait::async_trait]
pub trait ToolHandler: Send + Sync {
    /// Call a tool with the given name and arguments
    async fn call_tool(
        &self,
        name: &str,
        arguments: Option<serde_json::Value>,
    ) -> GrpcResult<CallToolResult>;
}

/// Trait for handling resource reads
#[async_trait::async_trait]
pub trait ResourceHandler: Send + Sync {
    /// Read a resource by URI
    async fn read_resource(&self, uri: &str) -> GrpcResult<Vec<ResourceContent>>;
}

/// Trait for handling prompt renders
#[async_trait::async_trait]
pub trait PromptHandler: Send + Sync {
    /// Get a prompt by name with arguments
    async fn get_prompt(
        &self,
        name: &str,
        arguments: Option<serde_json::Value>,
    ) -> GrpcResult<GetPromptResult>;
}

/// Default no-op tool handler
struct NoOpToolHandler;

#[async_trait::async_trait]
impl ToolHandler for NoOpToolHandler {
    async fn call_tool(
        &self,
        name: &str,
        _arguments: Option<serde_json::Value>,
    ) -> GrpcResult<CallToolResult> {
        Err(GrpcError::invalid_request(format!(
            "No handler for tool: {name}"
        )))
    }
}

/// Default no-op resource handler
struct NoOpResourceHandler;

#[async_trait::async_trait]
impl ResourceHandler for NoOpResourceHandler {
    async fn read_resource(&self, uri: &str) -> GrpcResult<Vec<ResourceContent>> {
        Err(GrpcError::invalid_request(format!(
            "No handler for resource: {uri}"
        )))
    }
}

/// Default no-op prompt handler
struct NoOpPromptHandler;

#[async_trait::async_trait]
impl PromptHandler for NoOpPromptHandler {
    async fn get_prompt(
        &self,
        name: &str,
        _arguments: Option<serde_json::Value>,
    ) -> GrpcResult<GetPromptResult> {
        Err(GrpcError::invalid_request(format!(
            "No handler for prompt: {name}"
        )))
    }
}

impl McpGrpcServer {
    /// Create a new server builder
    #[must_use]
    pub fn builder() -> McpGrpcServerBuilder {
        McpGrpcServerBuilder::new()
    }

    /// Get the tonic service for this server
    #[must_use]
    pub fn into_service(self) -> McpServiceServer<Self> {
        McpServiceServer::new(self)
    }

    /// Send a notification to all subscribers
    pub fn send_notification(&self, notification: proto::Notification) {
        let _ = self.notification_tx.send(notification);
    }

    /// Notify that the tool list has changed
    pub fn notify_tool_list_changed(&self) {
        self.send_notification(proto::Notification {
            notification: Some(proto::notification::Notification::ToolListChanged(
                proto::ToolListChangedNotification {},
            )),
        });
    }

    /// Notify that the resource list has changed
    pub fn notify_resource_list_changed(&self) {
        self.send_notification(proto::Notification {
            notification: Some(proto::notification::Notification::ResourceListChanged(
                proto::ResourceListChangedNotification {},
            )),
        });
    }

    /// Notify that the prompt list has changed
    pub fn notify_prompt_list_changed(&self) {
        self.send_notification(proto::Notification {
            notification: Some(proto::notification::Notification::PromptListChanged(
                proto::PromptListChangedNotification {},
            )),
        });
    }
}

/// Builder for `McpGrpcServer`
pub struct McpGrpcServerBuilder {
    server_info: Implementation,
    capabilities: ServerCapabilities,
    protocol_version: String,
    instructions: Option<String>,
    tools: Vec<Tool>,
    resources: Vec<Resource>,
    resource_templates: Vec<ResourceTemplate>,
    prompts: Vec<Prompt>,
    tool_handler: Option<Arc<dyn ToolHandler + Send + Sync>>,
    resource_handler: Option<Arc<dyn ResourceHandler + Send + Sync>>,
    prompt_handler: Option<Arc<dyn PromptHandler + Send + Sync>>,
}

impl McpGrpcServerBuilder {
    /// Create a new builder
    fn new() -> Self {
        Self {
            server_info: Implementation {
                name: "turbomcp-grpc".to_string(),
                title: None,
                description: None,
                version: env!("CARGO_PKG_VERSION").to_string(),
                icon: None,
            },
            capabilities: ServerCapabilities::default(),
            protocol_version: "2025-11-25".to_string(),
            instructions: None,
            tools: Vec::new(),
            resources: Vec::new(),
            resource_templates: Vec::new(),
            prompts: Vec::new(),
            tool_handler: None,
            resource_handler: None,
            prompt_handler: None,
        }
    }

    /// Set server name and version
    #[must_use]
    pub fn server_info(mut self, name: impl Into<String>, version: impl Into<String>) -> Self {
        self.server_info = Implementation {
            name: name.into(),
            title: None,
            description: None,
            version: version.into(),
            icon: None,
        };
        self
    }

    /// Set server capabilities
    #[must_use]
    pub fn capabilities(mut self, capabilities: ServerCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Set protocol version
    #[must_use]
    pub fn protocol_version(mut self, version: impl Into<String>) -> Self {
        self.protocol_version = version.into();
        self
    }

    /// Set server instructions
    #[must_use]
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    /// Add a tool
    #[must_use]
    pub fn add_tool(mut self, tool: Tool) -> Self {
        self.tools.push(tool);
        self
    }

    /// Add a resource
    #[must_use]
    pub fn add_resource(mut self, resource: Resource) -> Self {
        self.resources.push(resource);
        self
    }

    /// Add a resource template
    #[must_use]
    pub fn add_resource_template(mut self, template: ResourceTemplate) -> Self {
        self.resource_templates.push(template);
        self
    }

    /// Add a prompt
    #[must_use]
    pub fn add_prompt(mut self, prompt: Prompt) -> Self {
        self.prompts.push(prompt);
        self
    }

    /// Set the tool handler
    #[must_use]
    pub fn tool_handler<H: ToolHandler + 'static>(mut self, handler: H) -> Self {
        self.tool_handler = Some(Arc::new(handler));
        self
    }

    /// Set the resource handler
    #[must_use]
    pub fn resource_handler<H: ResourceHandler + 'static>(mut self, handler: H) -> Self {
        self.resource_handler = Some(Arc::new(handler));
        self
    }

    /// Set the prompt handler
    #[must_use]
    pub fn prompt_handler<H: PromptHandler + 'static>(mut self, handler: H) -> Self {
        self.prompt_handler = Some(Arc::new(handler));
        self
    }

    /// Build the server
    #[must_use]
    pub fn build(self) -> McpGrpcServer {
        let (notification_tx, _) = broadcast::channel(256);

        McpGrpcServer {
            server_info: self.server_info,
            capabilities: self.capabilities,
            protocol_version: self.protocol_version,
            instructions: self.instructions,
            tools: Arc::new(RwLock::new(self.tools)),
            resources: Arc::new(RwLock::new(self.resources)),
            resource_templates: Arc::new(RwLock::new(self.resource_templates)),
            prompts: Arc::new(RwLock::new(self.prompts)),
            notification_tx,
            tool_handler: self
                .tool_handler
                .unwrap_or_else(|| Arc::new(NoOpToolHandler)),
            resource_handler: self
                .resource_handler
                .unwrap_or_else(|| Arc::new(NoOpResourceHandler)),
            prompt_handler: self
                .prompt_handler
                .unwrap_or_else(|| Arc::new(NoOpPromptHandler)),
        }
    }
}

impl Default for McpGrpcServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[tonic::async_trait]
impl McpService for McpGrpcServer {
    #[instrument(skip(self, request), fields(method = "Initialize"))]
    async fn initialize(
        &self,
        request: Request<proto::InitializeRequest>,
    ) -> Result<Response<proto::InitializeResult>, Status> {
        let req = request.into_inner();
        info!(
            protocol_version = %req.protocol_version,
            client = ?req.client_info,
            "Initialize request"
        );

        let result = proto::InitializeResult {
            protocol_version: self.protocol_version.clone(),
            capabilities: Some(self.capabilities.clone().into()),
            server_info: Some(self.server_info.clone().into()),
            instructions: self.instructions.clone(),
        };

        Ok(Response::new(result))
    }

    #[instrument(skip(self, _request), fields(method = "Ping"))]
    async fn ping(
        &self,
        _request: Request<proto::PingRequest>,
    ) -> Result<Response<proto::PingResponse>, Status> {
        debug!("Ping");
        Ok(Response::new(proto::PingResponse {}))
    }

    #[instrument(skip(self, request), fields(method = "ListTools"))]
    async fn list_tools(
        &self,
        request: Request<proto::ListToolsRequest>,
    ) -> Result<Response<proto::ListToolsResult>, Status> {
        let _req = request.into_inner();
        debug!("ListTools");

        let tools = self.tools.read().await;
        let proto_tools: Result<Vec<_>, _> = tools.iter().cloned().map(TryInto::try_into).collect();

        Ok(Response::new(proto::ListToolsResult {
            tools: proto_tools.map_err(|e: GrpcError| Status::from(e))?,
            next_cursor: None,
        }))
    }

    #[instrument(skip(self, request), fields(method = "CallTool", tool = %request.get_ref().name))]
    async fn call_tool(
        &self,
        request: Request<proto::CallToolRequest>,
    ) -> Result<Response<proto::CallToolResult>, Status> {
        let req = request.into_inner();
        debug!(tool = %req.name, "CallTool");

        let arguments: Option<serde_json::Value> = if let Some(args) = req.arguments {
            if args.is_empty() {
                None
            } else {
                Some(
                    serde_json::from_slice(&args)
                        .map_err(|e| Status::invalid_argument(format!("Invalid arguments: {e}")))?,
                )
            }
        } else {
            None
        };

        let result = self
            .tool_handler
            .call_tool(&req.name, arguments)
            .await
            .map_err(Status::from)?;

        let proto_result: proto::CallToolResult = result.try_into().map_err(Status::from)?;
        Ok(Response::new(proto_result))
    }

    #[instrument(skip(self, request), fields(method = "ListResources"))]
    async fn list_resources(
        &self,
        request: Request<proto::ListResourcesRequest>,
    ) -> Result<Response<proto::ListResourcesResult>, Status> {
        let _req = request.into_inner();
        debug!("ListResources");

        let resources = self.resources.read().await;
        let proto_resources: Vec<_> = resources.iter().cloned().map(Into::into).collect();

        Ok(Response::new(proto::ListResourcesResult {
            resources: proto_resources,
            next_cursor: None,
        }))
    }

    #[instrument(skip(self, request), fields(method = "ListResourceTemplates"))]
    async fn list_resource_templates(
        &self,
        request: Request<proto::ListResourceTemplatesRequest>,
    ) -> Result<Response<proto::ListResourceTemplatesResult>, Status> {
        let _req = request.into_inner();
        debug!("ListResourceTemplates");

        let templates = self.resource_templates.read().await;
        let proto_templates: Vec<_> = templates.iter().cloned().map(Into::into).collect();

        Ok(Response::new(proto::ListResourceTemplatesResult {
            resource_templates: proto_templates,
            next_cursor: None,
        }))
    }

    #[instrument(skip(self, request), fields(method = "ReadResource", uri = %request.get_ref().uri))]
    async fn read_resource(
        &self,
        request: Request<proto::ReadResourceRequest>,
    ) -> Result<Response<proto::ReadResourceResult>, Status> {
        let req = request.into_inner();
        debug!(uri = %req.uri, "ReadResource");

        let contents = self
            .resource_handler
            .read_resource(&req.uri)
            .await
            .map_err(Status::from)?;

        let proto_contents: Result<Vec<_>, _> =
            contents.into_iter().map(TryInto::try_into).collect();

        Ok(Response::new(proto::ReadResourceResult {
            contents: proto_contents.map_err(|e: GrpcError| Status::from(e))?,
        }))
    }

    #[instrument(skip(self, request), fields(method = "ListPrompts"))]
    async fn list_prompts(
        &self,
        request: Request<proto::ListPromptsRequest>,
    ) -> Result<Response<proto::ListPromptsResult>, Status> {
        let _req = request.into_inner();
        debug!("ListPrompts");

        let prompts = self.prompts.read().await;
        let proto_prompts: Vec<_> = prompts.iter().cloned().map(Into::into).collect();

        Ok(Response::new(proto::ListPromptsResult {
            prompts: proto_prompts,
            next_cursor: None,
        }))
    }

    #[instrument(skip(self, request), fields(method = "GetPrompt", name = %request.get_ref().name))]
    async fn get_prompt(
        &self,
        request: Request<proto::GetPromptRequest>,
    ) -> Result<Response<proto::GetPromptResult>, Status> {
        let req = request.into_inner();
        debug!(name = %req.name, "GetPrompt");

        let arguments: Option<serde_json::Value> = if let Some(args) = req.arguments {
            if args.is_empty() {
                None
            } else {
                Some(
                    serde_json::from_slice(&args)
                        .map_err(|e| Status::invalid_argument(format!("Invalid arguments: {e}")))?,
                )
            }
        } else {
            None
        };

        let result = self
            .prompt_handler
            .get_prompt(&req.name, arguments)
            .await
            .map_err(Status::from)?;

        let proto_result: proto::GetPromptResult = result.try_into().map_err(Status::from)?;
        Ok(Response::new(proto_result))
    }

    #[instrument(skip(self, request), fields(method = "Complete"))]
    async fn complete(
        &self,
        request: Request<proto::CompleteRequest>,
    ) -> Result<Response<proto::CompleteResult>, Status> {
        let _req = request.into_inner();
        debug!("Complete");

        // Return empty completion - subclasses can override
        Ok(Response::new(proto::CompleteResult {
            completion: Some(proto::Completion {
                values: Vec::new(),
                total: None,
                has_more: Some(false),
            }),
        }))
    }

    type SubscribeStream = Pin<Box<dyn Stream<Item = Result<proto::Notification, Status>> + Send>>;

    #[instrument(skip(self, request), fields(method = "Subscribe"))]
    async fn subscribe(
        &self,
        request: Request<proto::SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let _req = request.into_inner();
        info!("Client subscribing to notifications");

        let mut rx = self.notification_tx.subscribe();

        let stream = async_stream::stream! {
            while let Ok(notification) = rx.recv().await {
                yield Ok(notification);
            }
        };

        Ok(Response::new(Box::pin(stream)))
    }

    #[instrument(skip(self, request), fields(method = "SetLoggingLevel"))]
    async fn set_logging_level(
        &self,
        request: Request<proto::SetLoggingLevelRequest>,
    ) -> Result<Response<proto::SetLoggingLevelResponse>, Status> {
        let req = request.into_inner();
        debug!(level = ?req.level, "SetLoggingLevel");

        // Logging level changes would be handled by the application
        Ok(Response::new(proto::SetLoggingLevelResponse {}))
    }

    #[instrument(skip(self, request), fields(method = "ListRoots"))]
    async fn list_roots(
        &self,
        request: Request<proto::ListRootsRequest>,
    ) -> Result<Response<proto::ListRootsResult>, Status> {
        let _req = request.into_inner();
        debug!("ListRoots");

        // Return empty roots - this is a client capability
        Ok(Response::new(proto::ListRootsResult { roots: Vec::new() }))
    }

    #[instrument(skip(self, request), fields(method = "CreateSamplingMessage"))]
    async fn create_sampling_message(
        &self,
        request: Request<proto::CreateSamplingMessageRequest>,
    ) -> Result<Response<proto::CreateSamplingMessageResult>, Status> {
        let _req = request.into_inner();

        // Sampling is a client capability, not typically implemented by servers
        Err(Status::unimplemented("Sampling is a client capability"))
    }

    #[instrument(skip(self, request), fields(method = "Elicit"))]
    async fn elicit(
        &self,
        request: Request<proto::ElicitRequest>,
    ) -> Result<Response<proto::ElicitResult>, Status> {
        let _req = request.into_inner();

        // Elicitation requires human interaction
        Err(Status::unimplemented(
            "Elicitation requires human interaction",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_builder() {
        let server = McpGrpcServer::builder()
            .server_info("test-server", "1.0.0")
            .protocol_version("2025-11-25")
            .instructions("Test server instructions")
            .add_tool(Tool {
                name: "test_tool".to_string(),
                description: Some("A test tool".to_string()),
                input_schema: turbomcp_core::types::tools::ToolInputSchema::default(),
                title: None,
                icon: None,
                annotations: None,
            })
            .build();

        assert_eq!(server.server_info.name, "test-server");
        assert_eq!(server.server_info.version, "1.0.0");
        assert_eq!(server.protocol_version, "2025-11-25");
    }
}
