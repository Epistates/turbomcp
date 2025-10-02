//! Function-based tool handler implementation

use async_trait::async_trait;
use std::sync::Arc;
use turbomcp_core::RequestContext;
use turbomcp_protocol::types::{CallToolRequest, CallToolResult, Tool};

use crate::ServerResult;
use crate::handlers::traits::ToolHandler;

type BoxFuture<T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>;

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
