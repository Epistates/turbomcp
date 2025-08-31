//! Handler traits and implementations for MCP operations

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use turbomcp_core::RequestContext;
use turbomcp_protocol::LogLevel;
use turbomcp_protocol::types::{
    CallToolRequest,
    CallToolResult,
    CompleteRequestParams,
    CompletionResponse,
    CreateMessageRequest,
    CreateMessageResult,
    // New MCP feature types
    ElicitRequest,
    ElicitResult,
    EmptyResult,
    GetPromptRequest,
    GetPromptResult,
    ListResourceTemplatesRequest,
    ListResourceTemplatesResult,
    LoggingCapabilities,
    PingRequest,
    PingResult,
    Prompt,
    ReadResourceRequest,
    ReadResourceResult,
    Resource,
    ResourceTemplate,
    SamplingCapabilities,
    SetLevelRequest,
    Tool,
    ToolInputSchema,
};

use crate::ServerResult;

/// Type alias for existence check functions to reduce complexity
type ExistenceCheckFn =
    Arc<dyn Fn(&str) -> futures::future::BoxFuture<'static, bool> + Send + Sync>;

/// Tool handler trait for processing tool calls
#[async_trait]
pub trait ToolHandler: Send + Sync {
    /// Handle a tool call request
    async fn handle(
        &self,
        request: CallToolRequest,
        ctx: RequestContext,
    ) -> ServerResult<CallToolResult>;

    /// Get the tool definition
    fn tool_definition(&self) -> Tool;

    /// Validate tool input (optional, default implementation allows all)
    fn validate_input(&self, _input: &Value) -> ServerResult<()> {
        Ok(())
    }

    /// Allowed roles for this tool (RBAC). None means unrestricted.
    fn allowed_roles(&self) -> Option<&[String]> {
        None
    }
}

/// Prompt handler trait for processing prompt requests
#[async_trait]
pub trait PromptHandler: Send + Sync {
    /// Handle a prompt request
    async fn handle(
        &self,
        request: GetPromptRequest,
        ctx: RequestContext,
    ) -> ServerResult<GetPromptResult>;

    /// Get the prompt definition
    fn prompt_definition(&self) -> Prompt;

    /// Validate prompt arguments (optional, default implementation allows all)
    fn validate_arguments(&self, _args: &HashMap<String, Value>) -> ServerResult<()> {
        Ok(())
    }
}

/// Resource handler trait for processing resource requests
#[async_trait]
pub trait ResourceHandler: Send + Sync {
    /// Handle a resource read request
    async fn handle(
        &self,
        request: ReadResourceRequest,
        ctx: RequestContext,
    ) -> ServerResult<ReadResourceResult>;

    /// Get the resource definition
    fn resource_definition(&self) -> Resource;

    /// Check if resource exists
    async fn exists(&self, uri: &str) -> bool;

    /// Get resource metadata
    async fn metadata(&self, _uri: &str) -> Option<HashMap<String, Value>> {
        None
    }
}

/// Sampling handler trait for processing sampling requests
#[async_trait]
pub trait SamplingHandler: Send + Sync {
    /// Handle a sampling request
    async fn handle(
        &self,
        request: CreateMessageRequest,
        ctx: RequestContext,
    ) -> ServerResult<CreateMessageResult>;

    /// Get supported sampling capabilities
    fn sampling_capabilities(&self) -> SamplingCapabilities {
        SamplingCapabilities
    }
}

/// Logging handler trait for processing logging requests
#[async_trait]
pub trait LoggingHandler: Send + Sync {
    /// Handle a log level change request
    async fn handle(
        &self,
        request: SetLevelRequest,
        ctx: RequestContext,
    ) -> ServerResult<EmptyResult>;

    /// Get current log level
    fn current_level(&self) -> LogLevel;

    /// Get logging capabilities
    fn logging_capabilities(&self) -> LoggingCapabilities {
        LoggingCapabilities
    }
}

// ============================================================================
// Enhanced Handler Traits for New MCP Features
// ============================================================================

/// Elicitation handler trait for server-initiated user input requests
#[async_trait]
pub trait ElicitationHandler: Send + Sync {
    /// Handle an elicitation request (server-initiated user input)
    async fn handle(
        &self,
        request: ElicitRequest,
        ctx: RequestContext,
    ) -> ServerResult<ElicitResult>;

    /// Validate elicitation schema (optional, default implementation allows all)
    fn validate_schema(&self, _schema: &serde_json::Value) -> ServerResult<()> {
        Ok(())
    }

    /// Get default timeout for user response in milliseconds
    fn default_timeout_ms(&self) -> u64 {
        60_000 // 1 minute default
    }

    /// Check if elicitation is cancellable
    fn is_cancellable(&self) -> bool {
        true
    }

    /// Handle elicitation cancellation
    async fn handle_cancellation(
        &self,
        _request_id: &str,
        _ctx: RequestContext,
    ) -> ServerResult<()> {
        Ok(())
    }

    /// Process user response for validation
    async fn process_response(
        &self,
        _response: &HashMap<String, serde_json::Value>,
        _schema: &serde_json::Value,
        _ctx: RequestContext,
    ) -> ServerResult<HashMap<String, serde_json::Value>> {
        // Default implementation returns response as-is
        Ok(_response.clone())
    }
}

/// Completion handler trait for argument autocompletion
#[async_trait]
pub trait CompletionHandler: Send + Sync {
    /// Handle a completion request
    async fn handle(
        &self,
        request: CompleteRequestParams,
        ctx: RequestContext,
    ) -> ServerResult<CompletionResponse>;

    /// Get maximum number of completions to return
    fn max_completions(&self) -> usize {
        50
    }

    /// Check if completion is supported for the given reference
    fn supports_completion(&self, _reference: &str) -> bool {
        true
    }

    /// Get completion suggestions based on context
    async fn get_completions(
        &self,
        reference: &str,
        argument: Option<&str>,
        partial_value: Option<&str>,
        ctx: RequestContext,
    ) -> ServerResult<Vec<serde_json::Value>>;

    /// Filter and rank completion options
    fn filter_completions(
        &self,
        completions: Vec<serde_json::Value>,
        partial_value: Option<&str>,
    ) -> Vec<serde_json::Value> {
        // Default implementation: simple prefix matching
        if let Some(partial) = partial_value {
            let partial_lower = partial.to_lowercase();
            completions
                .into_iter()
                .filter(|comp| {
                    if let Some(value) = comp.get("value").and_then(|v| v.as_str()) {
                        value.to_lowercase().starts_with(&partial_lower)
                    } else {
                        false
                    }
                })
                .take(self.max_completions())
                .collect()
        } else {
            completions
                .into_iter()
                .take(self.max_completions())
                .collect()
        }
    }
}

/// Resource template handler trait for parameterized resource access
#[async_trait]
pub trait ResourceTemplateHandler: Send + Sync {
    /// Handle a list resource templates request
    async fn handle(
        &self,
        request: ListResourceTemplatesRequest,
        ctx: RequestContext,
    ) -> ServerResult<ListResourceTemplatesResult>;

    /// Get available resource templates
    async fn get_templates(&self, ctx: RequestContext) -> ServerResult<Vec<ResourceTemplate>>;

    /// Get a specific template by name
    async fn get_template(
        &self,
        name: &str,
        ctx: RequestContext,
    ) -> ServerResult<Option<ResourceTemplate>>;

    /// Validate template URI pattern (RFC 6570)
    fn validate_uri_template(&self, uri_template: &str) -> ServerResult<()> {
        // Basic validation - can be overridden for more sophisticated checking
        if uri_template.is_empty() {
            return Err(crate::ServerError::Handler {
                message: "URI template cannot be empty".to_string(),
                context: None,
            });
        }
        Ok(())
    }

    /// Expand URI template with parameters
    fn expand_template(
        &self,
        uri_template: &str,
        parameters: &HashMap<String, serde_json::Value>,
    ) -> ServerResult<String> {
        // Basic template expansion - should be overridden for full RFC 6570 support
        let mut result = uri_template.to_string();

        for (key, value) in parameters {
            let placeholder = format!("{{{}}}", key);
            if let Some(str_value) = value.as_str() {
                result = result.replace(&placeholder, str_value);
            } else {
                result = result.replace(&placeholder, &value.to_string());
            }
        }

        Ok(result)
    }

    /// Validate template parameters
    async fn validate_parameters(
        &self,
        _template: &ResourceTemplate,
        _parameters: &HashMap<String, serde_json::Value>,
        _ctx: RequestContext,
    ) -> ServerResult<()> {
        // Default implementation - no validation
        // Override in implementations to add specific parameter validation
        Ok(())
    }
}

/// Ping handler trait for bidirectional health monitoring
#[async_trait]
pub trait PingHandler: Send + Sync {
    /// Handle a ping request
    async fn handle(&self, request: PingRequest, ctx: RequestContext) -> ServerResult<PingResult>;

    /// Get current health status
    async fn get_health_status(&self, _ctx: RequestContext) -> ServerResult<serde_json::Value> {
        Ok(serde_json::json!({
            "status": "healthy",
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }))
    }

    /// Get connection metrics if available
    async fn get_connection_metrics(
        &self,
        _ctx: RequestContext,
    ) -> ServerResult<Option<serde_json::Value>> {
        Ok(None) // Default: no metrics
    }

    /// Handle ping timeout
    async fn handle_timeout(&self, _request_id: &str, _ctx: RequestContext) -> ServerResult<()> {
        Ok(())
    }

    /// Check if ping should include detailed health information
    fn include_health_details(&self) -> bool {
        false
    }

    /// Get expected response time threshold in milliseconds
    fn response_threshold_ms(&self) -> u64 {
        5_000 // 5 seconds default
    }

    /// Process custom ping payload
    async fn process_ping_payload(
        &self,
        payload: Option<&serde_json::Value>,
        _ctx: RequestContext,
    ) -> ServerResult<Option<serde_json::Value>> {
        // Default: echo back the payload
        Ok(payload.cloned())
    }
}

/// Composite handler that can handle multiple types of requests
pub trait CompositeHandler: Send + Sync {
    /// Get tool handler if this composite handles tools
    fn as_tool_handler(&self) -> Option<&dyn ToolHandler> {
        None
    }

    /// Get prompt handler if this composite handles prompts
    fn as_prompt_handler(&self) -> Option<&dyn PromptHandler> {
        None
    }

    /// Get resource handler if this composite handles resources
    fn as_resource_handler(&self) -> Option<&dyn ResourceHandler> {
        None
    }

    /// Get sampling handler if this composite handles sampling
    fn as_sampling_handler(&self) -> Option<&dyn SamplingHandler> {
        None
    }

    /// Get logging handler if this composite handles logging
    fn as_logging_handler(&self) -> Option<&dyn LoggingHandler> {
        None
    }

    /// Get elicitation handler if this composite handles elicitation
    fn as_elicitation_handler(&self) -> Option<&dyn ElicitationHandler> {
        None
    }

    /// Get completion handler if this composite handles completion
    fn as_completion_handler(&self) -> Option<&dyn CompletionHandler> {
        None
    }

    /// Get resource template handler if this composite handles resource templates
    fn as_resource_template_handler(&self) -> Option<&dyn ResourceTemplateHandler> {
        None
    }

    /// Get ping handler if this composite handles ping
    fn as_ping_handler(&self) -> Option<&dyn PingHandler> {
        None
    }
}

/// Handler wrapper that provides additional functionality
pub struct HandlerWrapper<T> {
    /// The wrapped handler
    handler: Arc<T>,
    /// Handler metadata
    metadata: HandlerMetadata,
}

impl<T> std::fmt::Debug for HandlerWrapper<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HandlerWrapper")
            .field("metadata", &self.metadata)
            .finish()
    }
}

/// Metadata associated with a handler
#[derive(Debug, Clone)]
pub struct HandlerMetadata {
    /// Handler name
    pub name: String,
    /// Handler version
    pub version: String,
    /// Handler description
    pub description: Option<String>,
    /// Handler tags
    pub tags: Vec<String>,
    /// Handler creation time
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Handler configuration
    pub config: HashMap<String, Value>,
    /// Handler metrics enabled
    pub metrics_enabled: bool,
    /// Handler rate limit (requests per second)
    pub rate_limit: Option<u32>,
    /// Allowed roles for authorization (if None or empty => allow all)
    pub allowed_roles: Option<Vec<String>>,
}

impl Default for HandlerMetadata {
    fn default() -> Self {
        Self {
            name: "unnamed".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            tags: Vec::new(),
            created_at: chrono::Utc::now(),
            config: HashMap::new(),
            metrics_enabled: true,
            rate_limit: None,
            allowed_roles: None,
        }
    }
}

impl<T> HandlerWrapper<T> {
    /// Create a new handler wrapper
    pub fn new(handler: T) -> Self {
        Self {
            handler: Arc::new(handler),
            metadata: HandlerMetadata::default(),
        }
    }

    /// Create a wrapper with metadata
    pub fn with_metadata(handler: T, metadata: HandlerMetadata) -> Self {
        Self {
            handler: Arc::new(handler),
            metadata,
        }
    }

    /// Get handler reference
    #[must_use]
    pub const fn handler(&self) -> &Arc<T> {
        &self.handler
    }

    /// Get handler metadata
    #[must_use]
    pub const fn metadata(&self) -> &HandlerMetadata {
        &self.metadata
    }

    /// Update handler metadata
    pub fn update_metadata<F>(&mut self, f: F)
    where
        F: FnOnce(&mut HandlerMetadata),
    {
        f(&mut self.metadata);
    }
}

impl<T: Clone> Clone for HandlerWrapper<T> {
    fn clone(&self) -> Self {
        Self {
            handler: Arc::clone(&self.handler),
            metadata: self.metadata.clone(),
        }
    }
}

/// Function-based tool handler
pub struct FunctionToolHandler {
    /// Tool definition
    tool: Tool,
    /// Handler function
    handler: Arc<
        dyn Fn(CallToolRequest, RequestContext) -> BoxFuture<ServerResult<CallToolResult>>
            + Send
            + Sync,
    >,
    /// Allowed roles (RBAC)
    allowed_roles: Option<Vec<String>>,
}

impl std::fmt::Debug for FunctionToolHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FunctionToolHandler")
            .field("tool", &self.tool)
            .finish()
    }
}

type BoxFuture<T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>;

impl FunctionToolHandler {
    /// Create a new function-based tool handler
    pub fn new<F, Fut>(tool: Tool, handler: F) -> Self
    where
        F: Fn(CallToolRequest, RequestContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ServerResult<CallToolResult>> + Send + 'static,
    {
        Self::new_with_roles(tool, handler, None)
    }

    /// Create a new function-based tool handler with RBAC roles
    pub fn new_with_roles<F, Fut>(
        tool: Tool,
        handler: F,
        allowed_roles: Option<Vec<String>>,
    ) -> Self
    where
        F: Fn(CallToolRequest, RequestContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ServerResult<CallToolResult>> + Send + 'static,
    {
        let handler =
            Arc::new(move |req, ctx| Box::pin(handler(req, ctx)) as futures::future::BoxFuture<_>);
        Self {
            tool,
            handler,
            allowed_roles,
        }
    }
}

#[async_trait]
impl ToolHandler for FunctionToolHandler {
    async fn handle(
        &self,
        request: CallToolRequest,
        ctx: RequestContext,
    ) -> ServerResult<CallToolResult> {
        (self.handler)(request, ctx).await
    }

    fn tool_definition(&self) -> Tool {
        self.tool.clone()
    }

    fn allowed_roles(&self) -> Option<&[String]> {
        self.allowed_roles.as_deref()
    }
}

/// Function-based prompt handler
pub struct FunctionPromptHandler {
    /// Prompt definition
    prompt: Prompt,
    /// Handler function
    handler: Arc<
        dyn Fn(GetPromptRequest, RequestContext) -> BoxFuture<ServerResult<GetPromptResult>>
            + Send
            + Sync,
    >,
}

impl std::fmt::Debug for FunctionPromptHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FunctionPromptHandler")
            .field("prompt", &self.prompt)
            .finish()
    }
}

impl FunctionPromptHandler {
    /// Create a new function-based prompt handler
    pub fn new<F, Fut>(prompt: Prompt, handler: F) -> Self
    where
        F: Fn(GetPromptRequest, RequestContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ServerResult<GetPromptResult>> + Send + 'static,
    {
        let handler =
            Arc::new(move |req, ctx| Box::pin(handler(req, ctx)) as futures::future::BoxFuture<_>);
        Self { prompt, handler }
    }
}

#[async_trait]
impl PromptHandler for FunctionPromptHandler {
    async fn handle(
        &self,
        request: GetPromptRequest,
        ctx: RequestContext,
    ) -> ServerResult<GetPromptResult> {
        (self.handler)(request, ctx).await
    }

    fn prompt_definition(&self) -> Prompt {
        self.prompt.clone()
    }
}

/// Function-based resource handler
pub struct FunctionResourceHandler {
    /// Resource definition
    resource: Resource,
    /// Handler function
    handler: Arc<
        dyn Fn(ReadResourceRequest, RequestContext) -> BoxFuture<ServerResult<ReadResourceResult>>
            + Send
            + Sync,
    >,
    /// Existence check function
    exists_fn: ExistenceCheckFn,
}

impl std::fmt::Debug for FunctionResourceHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FunctionResourceHandler")
            .field("resource", &self.resource)
            .finish()
    }
}

impl FunctionResourceHandler {
    /// Create a new function-based resource handler
    pub fn new<F, Fut>(resource: Resource, handler: F) -> Self
    where
        F: Fn(ReadResourceRequest, RequestContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ServerResult<ReadResourceResult>> + Send + 'static,
    {
        let handler =
            Arc::new(move |req, ctx| Box::pin(handler(req, ctx)) as futures::future::BoxFuture<_>);
        let exists_fn = Arc::new(move |_uri: &str| {
            Box::pin(async move { true }) as futures::future::BoxFuture<'static, bool>
        });
        Self {
            resource,
            handler,
            exists_fn,
        }
    }
}

#[async_trait]
impl ResourceHandler for FunctionResourceHandler {
    async fn handle(
        &self,
        request: ReadResourceRequest,
        ctx: RequestContext,
    ) -> ServerResult<ReadResourceResult> {
        (self.handler)(request, ctx).await
    }

    fn resource_definition(&self) -> Resource {
        self.resource.clone()
    }

    async fn exists(&self, uri: &str) -> bool {
        (self.exists_fn)(uri).await
    }
}

/// Utility functions for creating handlers
pub mod utils {
    use super::{
        CallToolRequest,
        CallToolResult,
        CompleteRequestParams,
        CompletionResponse,
        // New MCP feature handler types
        ElicitRequest,
        ElicitResult,
        FunctionCompletionHandler,
        FunctionElicitationHandler,
        FunctionPingHandler,
        FunctionPromptHandler,
        FunctionResourceHandler,
        FunctionResourceTemplateHandler,
        FunctionToolHandler,
        GetPromptRequest,
        GetPromptResult,
        ListResourceTemplatesRequest,
        ListResourceTemplatesResult,
        PingRequest,
        PingResult,
        Prompt,
        ReadResourceRequest,
        ReadResourceResult,
        RequestContext,
        Resource,
        ResourceTemplate,
        ServerResult,
        Tool,
        ToolInputSchema,
    };

    /// Create a tool handler with complete metadata
    ///
    /// This provides a Tool specification with proper schema scaffolding
    /// that can be extended by macro-generated schemas when available.
    pub fn tool<F, Fut>(name: &str, description: &str, handler: F) -> FunctionToolHandler
    where
        F: Fn(CallToolRequest, RequestContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ServerResult<CallToolResult>> + Send + 'static,
    {
        let tool = Tool {
            name: name.to_string(),
            title: Some(name.to_string()),
            description: Some(description.to_string()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: Some(std::collections::HashMap::new()), // Extensible for macro schemas
                required: Some(Vec::new()), // Extensible for macro-generated required fields
                additional_properties: Some(false),
            },
            output_schema: None,
            annotations: None,
            meta: None,
        };
        FunctionToolHandler::new(tool, handler)
    }

    /// Create a tool handler with custom schema (used by macros)
    pub fn tool_with_schema<F, Fut>(
        name: &str,
        description: &str,
        schema: serde_json::Value,
        handler: F,
    ) -> FunctionToolHandler
    where
        F: Fn(CallToolRequest, RequestContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ServerResult<CallToolResult>> + Send + 'static,
    {
        // Extract properties, required, and additionalProperties from the schema
        let properties = schema
            .get("properties")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            serde_json::from_value(v.clone()).unwrap_or_default(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();

        let required = schema
            .get("required")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let additional_properties = schema
            .get("additionalProperties")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let tool = Tool {
            name: name.to_string(),
            title: Some(name.to_string()),
            description: Some(description.to_string()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: Some(properties),
                required: Some(required),
                additional_properties: Some(additional_properties),
            },
            output_schema: None,
            annotations: None,
            meta: None,
        };
        FunctionToolHandler::new(tool, handler)
    }

    /// Create a prompt handler with full specification
    pub fn prompt<F, Fut>(name: &str, description: &str, handler: F) -> FunctionPromptHandler
    where
        F: Fn(GetPromptRequest, RequestContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ServerResult<GetPromptResult>> + Send + 'static,
    {
        let prompt = Prompt {
            name: name.to_string(),
            title: Some(name.to_string()),
            description: Some(description.to_string()),
            arguments: None,
            meta: None,
        };
        FunctionPromptHandler::new(prompt, handler)
    }

    /// Create a resource handler with sensible defaults
    pub fn resource<F, Fut>(uri: &str, name: &str, handler: F) -> FunctionResourceHandler
    where
        F: Fn(ReadResourceRequest, RequestContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ServerResult<ReadResourceResult>> + Send + 'static,
    {
        let resource = Resource {
            name: name.to_string(),
            title: Some(name.to_string()),
            uri: uri.to_string(),
            description: None,
            mime_type: Some("text/plain".to_string()), // Sensible default for most resources
            annotations: None,
            size: None,
            meta: None,
        };
        FunctionResourceHandler::new(resource, handler)
    }

    // ========================================================================
    // Enhanced Handler Factory Functions for New MCP Features
    // ========================================================================

    /// Create an elicitation handler with schema validation
    pub fn elicitation<F, Fut>(schema: serde_json::Value, handler: F) -> FunctionElicitationHandler
    where
        F: Fn(ElicitRequest, RequestContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ServerResult<ElicitResult>> + Send + 'static,
    {
        FunctionElicitationHandler::new(schema, handler)
    }

    /// Create a completion handler for autocompletion
    pub fn completion<F, Fut>(handler: F) -> FunctionCompletionHandler
    where
        F: Fn(CompleteRequestParams, RequestContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ServerResult<CompletionResponse>> + Send + 'static,
    {
        FunctionCompletionHandler::new(handler)
    }

    /// Create a resource template handler with templates
    pub fn resource_template<F, Fut>(
        templates: Vec<ResourceTemplate>,
        handler: F,
    ) -> FunctionResourceTemplateHandler
    where
        F: Fn(ListResourceTemplatesRequest, RequestContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ServerResult<ListResourceTemplatesResult>>
            + Send
            + 'static,
    {
        FunctionResourceTemplateHandler::new(templates, handler)
    }

    /// Create a ping handler for health monitoring
    pub fn ping<F, Fut>(handler: F) -> FunctionPingHandler
    where
        F: Fn(PingRequest, RequestContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ServerResult<PingResult>> + Send + 'static,
    {
        FunctionPingHandler::new(handler)
    }
}

// ============================================================================
// Function-based Handler Implementations for New MCP Features
// ============================================================================

/// Function-based elicitation handler
pub struct FunctionElicitationHandler {
    /// Elicitation schema
    schema: serde_json::Value,
    /// Handler function
    handler: Arc<
        dyn Fn(ElicitRequest, RequestContext) -> BoxFuture<ServerResult<ElicitResult>>
            + Send
            + Sync,
    >,
}

impl std::fmt::Debug for FunctionElicitationHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FunctionElicitationHandler")
            .field("schema", &self.schema)
            .finish()
    }
}

impl FunctionElicitationHandler {
    /// Create a new function-based elicitation handler
    pub fn new<F, Fut>(schema: serde_json::Value, handler: F) -> Self
    where
        F: Fn(ElicitRequest, RequestContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ServerResult<ElicitResult>> + Send + 'static,
    {
        let handler =
            Arc::new(move |req, ctx| Box::pin(handler(req, ctx)) as futures::future::BoxFuture<_>);
        Self { schema, handler }
    }
}

#[async_trait]
impl ElicitationHandler for FunctionElicitationHandler {
    async fn handle(
        &self,
        request: ElicitRequest,
        ctx: RequestContext,
    ) -> ServerResult<ElicitResult> {
        (self.handler)(request, ctx).await
    }

    fn validate_schema(&self, schema: &serde_json::Value) -> ServerResult<()> {
        if schema != &self.schema {
            return Err(crate::ServerError::Handler {
                message: "Schema mismatch".to_string(),
                context: Some("elicitation_validation".to_string()),
            });
        }
        Ok(())
    }
}

/// Function-based completion handler
pub struct FunctionCompletionHandler {
    /// Handler function
    handler: Arc<
        dyn Fn(CompleteRequestParams, RequestContext) -> BoxFuture<ServerResult<CompletionResponse>>
            + Send
            + Sync,
    >,
}

impl std::fmt::Debug for FunctionCompletionHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FunctionCompletionHandler").finish()
    }
}

impl FunctionCompletionHandler {
    /// Create a new function-based completion handler
    pub fn new<F, Fut>(handler: F) -> Self
    where
        F: Fn(CompleteRequestParams, RequestContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ServerResult<CompletionResponse>> + Send + 'static,
    {
        let handler =
            Arc::new(move |req, ctx| Box::pin(handler(req, ctx)) as futures::future::BoxFuture<_>);
        Self { handler }
    }
}

#[async_trait]
impl CompletionHandler for FunctionCompletionHandler {
    async fn handle(
        &self,
        request: CompleteRequestParams,
        ctx: RequestContext,
    ) -> ServerResult<CompletionResponse> {
        (self.handler)(request, ctx).await
    }

    async fn get_completions(
        &self,
        _reference: &str,
        _argument: Option<&str>,
        _partial_value: Option<&str>,
        _ctx: RequestContext,
    ) -> ServerResult<Vec<serde_json::Value>> {
        // Default implementation - should be overridden
        Ok(Vec::new())
    }
}

/// Function-based resource template handler
pub struct FunctionResourceTemplateHandler {
    /// Available templates
    templates: Vec<ResourceTemplate>,
    /// Handler function
    handler: Arc<
        dyn Fn(
                ListResourceTemplatesRequest,
                RequestContext,
            ) -> BoxFuture<ServerResult<ListResourceTemplatesResult>>
            + Send
            + Sync,
    >,
}

impl std::fmt::Debug for FunctionResourceTemplateHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FunctionResourceTemplateHandler")
            .field("templates", &self.templates.len())
            .finish()
    }
}

impl FunctionResourceTemplateHandler {
    /// Create a new function-based resource template handler
    pub fn new<F, Fut>(templates: Vec<ResourceTemplate>, handler: F) -> Self
    where
        F: Fn(ListResourceTemplatesRequest, RequestContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ServerResult<ListResourceTemplatesResult>>
            + Send
            + 'static,
    {
        let handler =
            Arc::new(move |req, ctx| Box::pin(handler(req, ctx)) as futures::future::BoxFuture<_>);
        Self { templates, handler }
    }
}

#[async_trait]
impl ResourceTemplateHandler for FunctionResourceTemplateHandler {
    async fn handle(
        &self,
        request: ListResourceTemplatesRequest,
        ctx: RequestContext,
    ) -> ServerResult<ListResourceTemplatesResult> {
        (self.handler)(request, ctx).await
    }

    async fn get_templates(&self, _ctx: RequestContext) -> ServerResult<Vec<ResourceTemplate>> {
        Ok(self.templates.clone())
    }

    async fn get_template(
        &self,
        name: &str,
        _ctx: RequestContext,
    ) -> ServerResult<Option<ResourceTemplate>> {
        Ok(self.templates.iter().find(|t| t.name == name).cloned())
    }
}

/// Function-based ping handler
pub struct FunctionPingHandler {
    /// Handler function
    handler: Arc<
        dyn Fn(PingRequest, RequestContext) -> BoxFuture<ServerResult<PingResult>> + Send + Sync,
    >,
}

impl std::fmt::Debug for FunctionPingHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FunctionPingHandler").finish()
    }
}

impl FunctionPingHandler {
    /// Create a new function-based ping handler
    pub fn new<F, Fut>(handler: F) -> Self
    where
        F: Fn(PingRequest, RequestContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ServerResult<PingResult>> + Send + 'static,
    {
        let handler =
            Arc::new(move |req, ctx| Box::pin(handler(req, ctx)) as futures::future::BoxFuture<_>);
        Self { handler }
    }
}

#[async_trait]
impl PingHandler for FunctionPingHandler {
    async fn handle(&self, request: PingRequest, ctx: RequestContext) -> ServerResult<PingResult> {
        (self.handler)(request, ctx).await
    }
}
