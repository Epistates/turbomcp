//! Utility functions for creating handlers from closures
//!
//! This module provides convenience functions for creating handler implementations
//! from simple closures. These are primarily used by the `#[server]` macro but
//! can also be used directly for quick handler creation.

use std::collections::HashMap;
use turbomcp_protocol::RequestContext;
use turbomcp_protocol::types::{
    CallToolRequest, CallToolResult, GetPromptRequest, GetPromptResult, Prompt,
    ReadResourceRequest, ReadResourceResult, Resource, Tool, ToolInputSchema,
};

use crate::ServerResult;
use crate::handlers::implementations::FunctionToolHandler;
use crate::handlers::traits::{PromptHandler, ResourceHandler, ToolHandler};

/// Create a tool handler from a closure
///
/// This is a convenience function for creating simple tool handlers without
/// manually constructing Tool definitions.
///
/// # Arguments
///
/// * `name` - Tool name
/// * `description` - Tool description
/// * `handler` - Async closure that handles tool calls
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_server::handlers::utils::tool;
/// use turbomcp_protocol::RequestContext;
/// use turbomcp_protocol::types::{CallToolRequest, CallToolResult, Content, TextContent};
///
/// let my_tool = tool("echo", "Echoes back the input", |req: CallToolRequest, _ctx: RequestContext| async move {
///     Ok(CallToolResult {
///         content: vec![Content::Text(TextContent {
///             text: format!("Echo: {:?}", req.arguments),
///             annotations: None,
///             meta: None,
///         })],
///         is_error: None,
///         structured_content: None,
///         _meta: None,
///         task_id: None,
///     })
/// });
/// ```
pub fn tool<F, Fut>(
    name: impl Into<String>,
    description: impl Into<String>,
    handler: F,
) -> impl ToolHandler
where
    F: Fn(CallToolRequest, RequestContext) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ServerResult<CallToolResult>> + Send + 'static,
{
    let name = name.into();
    let description = description.into();

    #[allow(clippy::needless_update)] // Default needed for feature-gated fields (icons)
    let tool_def = Tool {
        name: name.clone(),
        title: Some(name),
        description: Some(description),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: Some(HashMap::new()),
            required: None,
            additional_properties: None,
        },
        output_schema: None,
        execution: None,
        annotations: None,
        meta: None,
        ..Default::default()
    };

    FunctionToolHandler::new(tool_def, handler)
}

/// Create a tool handler with a custom schema
///
/// This allows specifying the input schema for the tool, which is used by
/// the `#[server]` macro to provide type-safe tool definitions.
///
/// # Arguments
///
/// * `name` - Tool name
/// * `description` - Tool description
/// * `schema` - Input schema for the tool
/// * `handler` - Async closure that handles tool calls
pub fn tool_with_schema<F, Fut>(
    name: impl Into<String>,
    description: impl Into<String>,
    schema: ToolInputSchema,
    handler: F,
) -> impl ToolHandler
where
    F: Fn(CallToolRequest, RequestContext) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ServerResult<CallToolResult>> + Send + 'static,
{
    let name = name.into();
    let description = description.into();

    #[allow(clippy::needless_update)] // Default needed for feature-gated fields (icons)
    let tool_def = Tool {
        name: name.clone(),
        title: Some(name),
        description: Some(description),
        input_schema: schema,
        output_schema: None,
        execution: None,
        annotations: None,
        meta: None,
        ..Default::default()
    };

    FunctionToolHandler::new(tool_def, handler)
}

/// Function-based prompt handler
pub struct FunctionPromptHandler {
    prompt: Prompt,
    handler: Box<
        dyn Fn(
                GetPromptRequest,
                RequestContext,
            ) -> futures::future::BoxFuture<'static, ServerResult<GetPromptResult>>
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
    /// Create new prompt handler
    pub fn new<F, Fut>(prompt: Prompt, handler: F) -> Self
    where
        F: Fn(GetPromptRequest, RequestContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ServerResult<GetPromptResult>> + Send + 'static,
    {
        Self {
            prompt,
            handler: Box::new(move |req, ctx| Box::pin(handler(req, ctx))),
        }
    }
}

#[async_trait::async_trait]
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

/// Create a prompt handler from a closure
///
/// # Arguments
///
/// * `name` - Prompt name
/// * `description` - Prompt description
/// * `handler` - Async closure that handles prompt requests
pub fn prompt<F, Fut>(
    name: impl Into<String>,
    description: impl Into<String>,
    handler: F,
) -> impl PromptHandler
where
    F: Fn(GetPromptRequest, RequestContext) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ServerResult<GetPromptResult>> + Send + 'static,
{
    let name = name.into();
    let description = description.into();

    let prompt_def = Prompt {
        name: name.clone(),
        title: Some(name),
        description: Some(description),
        arguments: None,
        meta: None,
    };

    FunctionPromptHandler::new(prompt_def, handler)
}

/// Function-based resource handler
pub struct FunctionResourceHandler {
    resource: Resource,
    handler: Box<
        dyn Fn(
                ReadResourceRequest,
                RequestContext,
            ) -> futures::future::BoxFuture<'static, ServerResult<ReadResourceResult>>
            + Send
            + Sync,
    >,
}

impl std::fmt::Debug for FunctionResourceHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FunctionResourceHandler")
            .field("resource", &self.resource)
            .finish()
    }
}

impl FunctionResourceHandler {
    /// Create new resource handler
    pub fn new<F, Fut>(resource: Resource, handler: F) -> Self
    where
        F: Fn(ReadResourceRequest, RequestContext) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ServerResult<ReadResourceResult>> + Send + 'static,
    {
        Self {
            resource,
            handler: Box::new(move |req, ctx| Box::pin(handler(req, ctx))),
        }
    }
}

#[async_trait::async_trait]
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

    async fn exists(&self, _uri: &str) -> bool {
        true // Default implementation
    }
}

/// Create a resource handler from a closure
///
/// # Arguments
///
/// * `uri` - Resource URI
/// * `name` - Resource name
/// * `handler` - Async closure that handles resource read requests
pub fn resource<F, Fut>(
    uri: impl Into<String>,
    name: impl Into<String>,
    handler: F,
) -> impl ResourceHandler
where
    F: Fn(ReadResourceRequest, RequestContext) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ServerResult<ReadResourceResult>> + Send + 'static,
{
    let uri = uri.into();
    let name = name.into();

    let resource_def = Resource {
        name: name.clone(),
        title: Some(name),
        uri,
        description: None,
        mime_type: Some("text/plain".to_string()),
        annotations: None,
        size: None,
        meta: None,
    };

    FunctionResourceHandler::new(resource_def, handler)
}
