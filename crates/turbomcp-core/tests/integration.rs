// These implementations intentionally use `impl Future` syntax to match the McpHandler trait's
// RPITIT (Return Position Impl Trait In Trait) signatures for trait object safety.
#![allow(clippy::manual_async_fn)]

use turbomcp_core::{McpHandler, RequestContext, McpResult, McpError};
use turbomcp_types::{ServerInfo, Tool, ToolResult, Resource, ResourceResult, Prompt, PromptResult};
use serde_json::Value;
use std::future::Future;

// Mock Handler to verify trait object safety and basic dispatch
#[derive(Clone)]
struct MockHandler;

impl McpHandler for MockHandler {
    fn server_info(&self) -> ServerInfo {
        ServerInfo::new("mock-server", "1.0.0")
    }

    fn list_tools(&self) -> Vec<Tool> {
        vec![Tool::new("ping", "Ping pong")]
    }

    fn list_resources(&self) -> Vec<Resource> {
        vec![]
    }

    fn list_prompts(&self) -> Vec<Prompt> {
        vec![]
    }

    fn call_tool<'a>(
        &'a self,
        name: &'a str,
        _args: Value,
        _ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<ToolResult>> + Send + 'a {
        let name = name.to_string();
        async move {
            match name.as_str() {
                "ping" => Ok(ToolResult::text("pong")),
                _ => Err(McpError::tool_not_found(&name)),
            }
        }
    }

    fn read_resource<'a>(
        &'a self,
        uri: &'a str,
        _ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<ResourceResult>> + Send + 'a {
        async move { Err(McpError::resource_not_found(uri)) }
    }

    fn get_prompt<'a>(
        &'a self,
        name: &'a str,
        _args: Option<Value>,
        _ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<PromptResult>> + Send + 'a {
        async move { Err(McpError::prompt_not_found(name)) }
    }
}

#[tokio::test]
async fn test_handler_dispatch() {
    let handler = MockHandler;
    let ctx = RequestContext::default();

    // Test server info
    let info = handler.server_info();
    assert_eq!(info.name, "mock-server");

    // Test tool listing
    let tools = handler.list_tools();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "ping");

    // Test tool call (success)
    let result = handler.call_tool("ping", Value::Null, &ctx).await.unwrap();
    assert!(!result.is_error());
    
    // Test tool call (failure)
    let err = handler.call_tool("unknown", Value::Null, &ctx).await.unwrap_err();
    assert_eq!(err.jsonrpc_code(), -32001); // Tool not found code
}
