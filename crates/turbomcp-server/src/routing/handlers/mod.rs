//! Protocol handlers for MCP methods

pub mod completion;
pub mod elicitation;
pub mod initialize;
pub mod logging;
pub mod ping;
pub mod prompts;
pub mod resources;
pub mod roots;
pub mod sampling;
#[cfg(feature = "mcp-tasks")]
pub mod tasks;
pub mod tools;

use turbomcp_protocol::RequestContext;
use turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcResponse};

use crate::registry::HandlerRegistry;
use std::sync::Arc;

/// Handler context passed to all protocol handlers
/// Contains registry and server config for protocol responses
pub struct HandlerContext {
    pub registry: Arc<HandlerRegistry>,
    pub config: crate::config::ServerConfig,
    #[cfg(feature = "mcp-tasks")]
    pub task_storage: Arc<crate::task_storage::TaskStorage>,
}

impl HandlerContext {
    pub fn new(
        registry: Arc<HandlerRegistry>,
        config: crate::config::ServerConfig,
        #[cfg(feature = "mcp-tasks")] task_storage: Arc<crate::task_storage::TaskStorage>,
    ) -> Self {
        Self {
            registry,
            config,
            #[cfg(feature = "mcp-tasks")]
            task_storage,
        }
    }
}

/// Protocol handler dispatcher
pub struct ProtocolHandlers {
    context: HandlerContext,
}

impl ProtocolHandlers {
    pub fn new(context: HandlerContext) -> Self {
        Self { context }
    }

    /// Handle initialize request
    pub async fn handle_initialize(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        initialize::handle(&self.context, request, ctx).await
    }

    /// Handle list tools request
    pub async fn handle_list_tools(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        tools::handle_list(&self.context, request, ctx).await
    }

    /// Handle call tool request
    pub async fn handle_call_tool(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        tools::handle_call(&self.context, request, ctx).await
    }

    /// Handle list prompts request
    pub async fn handle_list_prompts(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        prompts::handle_list(&self.context, request, ctx).await
    }

    /// Handle get prompt request
    pub async fn handle_get_prompt(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        prompts::handle_get(&self.context, request, ctx).await
    }

    /// Handle list resources request
    pub async fn handle_list_resources(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        resources::handle_list(&self.context, request, ctx).await
    }

    /// Handle read resource request
    pub async fn handle_read_resource(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        resources::handle_read(&self.context, request, ctx).await
    }

    /// Handle subscribe resource request
    pub async fn handle_subscribe_resource(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        resources::handle_subscribe(&self.context, request, ctx).await
    }

    /// Handle unsubscribe resource request
    pub async fn handle_unsubscribe_resource(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        resources::handle_unsubscribe(&self.context, request, ctx).await
    }

    /// Handle set log level request
    pub async fn handle_set_log_level(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        logging::handle_set_level(&self.context, request, ctx).await
    }

    /// Handle create message request
    pub async fn handle_create_message(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        sampling::handle_create_message(&self.context, request, ctx).await
    }

    /// Handle list roots request
    pub async fn handle_list_roots(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        roots::handle_list(&self.context, request, ctx).await
    }

    /// Handle elicitation request
    pub async fn handle_elicitation(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        elicitation::handle(&self.context, request, ctx).await
    }

    /// Handle completion request
    pub async fn handle_completion(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        completion::handle(&self.context, request, ctx).await
    }

    /// Handle list resource templates request
    pub async fn handle_list_resource_templates(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        resources::handle_list_templates(&self.context, request, ctx).await
    }

    /// Handle ping request
    pub async fn handle_ping(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        ping::handle(&self.context, request, ctx).await
    }

    /// Handle tasks/get request - retrieve task status (MCP Tasks API - SEP-1686)
    #[cfg(feature = "mcp-tasks")]
    pub async fn handle_get_task(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        tasks::handle_get(&self.context, request, ctx).await
    }

    /// Handle tasks/result request - get task result (MCP Tasks API - SEP-1686)
    #[cfg(feature = "mcp-tasks")]
    pub async fn handle_task_result(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        tasks::handle_result(&self.context, request, ctx).await
    }

    /// Handle tasks/list request - list all tasks (MCP Tasks API - SEP-1686)
    #[cfg(feature = "mcp-tasks")]
    pub async fn handle_list_tasks(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        tasks::handle_list(&self.context, request, ctx).await
    }

    /// Handle tasks/cancel request - cancel a running task (MCP Tasks API - SEP-1686)
    #[cfg(feature = "mcp-tasks")]
    pub async fn handle_cancel_task(
        &self,
        request: JsonRpcRequest,
        ctx: RequestContext,
    ) -> JsonRpcResponse {
        tasks::handle_cancel(&self.context, request, ctx).await
    }
}
