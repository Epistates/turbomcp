//! Tool handler trait for processing tool calls

use async_trait::async_trait;
use serde_json::Value;
use turbomcp_protocol::RequestContext;
use turbomcp_protocol::types::{CallToolRequest, CallToolResult, Tool};

use crate::ServerResult;

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
