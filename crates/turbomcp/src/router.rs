//! `TurboMCP` Tool Router - Ergonomic wrapper over mcp-server routing
//!
//! Provides comprehensive tool router API while leveraging the robust
//! `mcp-server::routing` infrastructure for actual implementation.

//use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

/// Type alias for wrapped tool handler function
type WrappedToolHandler = Arc<
    dyn Fn(
            serde_json::Value,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = McpResult<turbomcp_protocol::types::CallToolResult>,
                    > + Send,
            >,
        > + Send
        + Sync,
>;
//use serde::{Deserialize, Serialize};

// Re-export core routing functionality from mcp-server
pub use turbomcp_server::{HandlerRegistry, RequestRouter, ToolHandler as ServerToolHandler};

use crate::{McpError, McpResult};
use turbomcp_protocol::RequestContext;
use turbomcp_protocol::{
    jsonrpc::{JsonRpcRequest, JsonRpcVersion},
    types::{CallToolRequest, CallToolResult, Tool, ToolInputSchema},
};

/// Ergonomic tool router that wraps mcp-server functionality
///
/// This provides a comprehensive API while using the well-established
/// `mcp-server::RequestRouter` under the hood.
pub struct ToolRouter<T> {
    /// Server instance (reserved for future use)
    #[allow(dead_code)]
    server: Arc<T>,
    /// Underlying mcp-server router
    inner_router: Arc<RequestRouter>,
    /// Handler registry from mcp-server
    registry: Arc<HandlerRegistry>,
}

impl<T> ToolRouter<T>
where
    T: Send + Sync + 'static,
{
    /// Create a new tool router
    pub fn new(server: T) -> Self {
        let registry = Arc::new(HandlerRegistry::new());
        let metrics = Arc::new(turbomcp_server::metrics::ServerMetrics::new());
        let router = Arc::new(RequestRouter::new(
            registry.clone(),
            metrics,
            turbomcp_server::config::ServerConfig::default(),
        ));

        Self {
            server: Arc::new(server),
            inner_router: router,
            registry,
        }
    }

    /// Register a tool handler (delegates to mcp-server)
    pub fn register_tool<F, Fut>(&self, name: String, handler: F) -> McpResult<()>
    where
        F: Fn(serde_json::Value) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = McpResult<CallToolResult>> + Send + 'static,
    {
        // Create wrapper that implements mcp-server::ToolHandler
        let wrapper = TurboToolHandlerWrapper {
            handler: Arc::new(move |args| Box::pin(handler(args))),
            tool_name: name.clone(),
        };

        self.registry
            .register_tool(name, wrapper)
            .map_err(|e| McpError::Tool(format!("Registration failed: {e}")))
    }

    /// List all registered tools (delegates to mcp-server)
    #[must_use]
    pub fn list_tools(&self) -> Vec<String> {
        self.registry.list_tools()
    }

    /// Execute a tool (delegates to mcp-server router)
    pub async fn call_tool(
        &self,
        tool_name: String,
        arguments: serde_json::Value,
        context: RequestContext,
    ) -> McpResult<CallToolResult> {
        // Convert to mcp-server request format
        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: turbomcp_protocol::MessageId::String(context.request_id.clone()),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": tool_name,
                "arguments": arguments
            })),
        };

        // Use mcp-server router (correct method name is 'route')
        let response = self.inner_router.route(request, context).await;

        // Convert response back to CallToolResult
        if let Some(result) = response.result() {
            serde_json::from_value(result.clone())
                .map_err(|e| McpError::Tool(format!("Response parsing failed: {e}")))
        } else if let Some(error) = response.error() {
            Err(McpError::Tool(format!(
                "Tool execution failed: {}",
                error.message
            )))
        } else {
            Err(McpError::Tool("Invalid response format".to_string()))
        }
    }

    /// Get the underlying mcp-server registry
    #[must_use]
    pub fn registry(&self) -> Arc<HandlerRegistry> {
        self.registry.clone()
    }

    /// Get the underlying mcp-server router
    #[must_use]
    pub fn router(&self) -> Arc<RequestRouter> {
        self.inner_router.clone()
    }
}

/// Wrapper to adapt `TurboMCP` handlers to mcp-server interface
struct TurboToolHandlerWrapper {
    handler: WrappedToolHandler,
    tool_name: String,
}

#[async_trait]
impl ServerToolHandler for TurboToolHandlerWrapper {
    async fn handle(
        &self,
        request: CallToolRequest,
        _context: RequestContext,
    ) -> turbomcp_server::ServerResult<CallToolResult> {
        let args = request.arguments.unwrap_or_default();
        let args_value = serde_json::to_value(args)
            .map_err(|e| turbomcp_server::ServerError::Internal(e.to_string()))?;
        match (self.handler)(args_value).await {
            Ok(result) => Ok(result),
            Err(e) => Err(turbomcp_server::ServerError::Internal(e.to_string())),
        }
    }

    fn tool_definition(&self) -> Tool {
        Tool {
            name: self.tool_name.clone(),
            title: None,
            description: Some("Tool handler".to_string()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: None,
                required: None,
                additional_properties: None,
            },
            output_schema: None,
            annotations: None,
            meta: None,
        }
    }
}

/// Ergonomic macro for creating tool routers
#[macro_export]
macro_rules! tool_router {
    ($server:expr) => {{ $crate::router::ToolRouter::new($server) }};
}

/// Convenience trait for adding comprehensive composition
pub trait ToolRouterExt {
    /// Combine multiple routers (delegates to mcp-server)
    fn combine(routers: Vec<Self>) -> McpResult<Self>
    where
        Self: Sized;
}

impl<T> ToolRouterExt for ToolRouter<T>
where
    T: Send + Sync + Default + 'static,
{
    fn combine(routers: Vec<Self>) -> McpResult<Self> {
        if routers.is_empty() {
            return Err(McpError::Tool(
                "Cannot combine empty router list".to_string(),
            ));
        }

        // Create combined registry by merging all router registries
        let combined_registry = Arc::new(HandlerRegistry::new());
        let metrics = Arc::new(turbomcp_server::metrics::ServerMetrics::new());
        let combined_router = Arc::new(RequestRouter::new(
            combined_registry.clone(),
            metrics,
            turbomcp_server::config::ServerConfig::default(),
        ));

        // Get the first router's server instance to use as the combined server
        let first_server = routers[0].server.clone();

        // For each router, merge its handlers into the combined registry
        for router in &routers {
            // Iterate over all tools and copy them to the combined registry
            for entry in &router.registry.tools {
                let (name, handler) = entry.pair();
                combined_registry
                    .tools
                    .insert(name.clone(), handler.clone());
            }

            // Iterate over all prompts and copy them to the combined registry
            for entry in &router.registry.prompts {
                let (name, handler) = entry.pair();
                combined_registry
                    .prompts
                    .insert(name.clone(), handler.clone());
            }

            // Iterate over all resources and copy them to the combined registry
            for entry in &router.registry.resources {
                let (template, handler) = entry.pair();
                combined_registry
                    .resources
                    .insert(template.clone(), handler.clone());
            }

            // Also copy sampling and logging handlers
            for entry in &router.registry.sampling {
                let (name, handler) = entry.pair();
                combined_registry
                    .sampling
                    .insert(name.clone(), handler.clone());
            }

            for entry in &router.registry.logging {
                let (name, handler) = entry.pair();
                combined_registry
                    .logging
                    .insert(name.clone(), handler.clone());
            }
        }

        Ok(Self {
            server: first_server,
            inner_router: combined_router,
            registry: combined_registry,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TestServer;

    #[tokio::test]
    async fn test_router_creation() {
        let router = ToolRouter::new(TestServer);
        let tools = router.list_tools();
        assert!(tools.is_empty());
    }

    #[tokio::test]
    async fn test_tool_registration() {
        let router = ToolRouter::new(TestServer);

        let result = router.register_tool("test_tool".to_string(), |_args| async {
            Ok(CallToolResult {
                content: vec![turbomcp_protocol::types::Content::Text(
                    turbomcp_protocol::types::TextContent {
                        text: "Hello".to_string(),
                        annotations: None,
                        meta: None,
                    },
                )],
                is_error: None,
                structured_content: None,
                _meta: None,
            })
        });

        assert!(result.is_ok());

        let tools = router.list_tools();
        assert!(tools.contains(&"test_tool".to_string()));
    }
}
