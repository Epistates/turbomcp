//! Manual Server Example
//!
//! This example demonstrates how to build an MCP server without macros,
//! useful for understanding the internal machinery or for dynamic server construction.

use std::future::Future;
use serde_json::{json, Value};
use turbomcp_server::{McpHandler, McpHandlerExt};
use turbomcp_core::{McpResult, McpError, RequestContext};
use turbomcp_types::{
    Tool, ToolResult, Resource, Prompt, ServerInfo,
    ResourceResult, PromptResult, ToolInputSchema
};

#[derive(Clone)]
struct ManualServer;

impl McpHandler for ManualServer {
    fn server_info(&self) -> ServerInfo {
        ServerInfo::new("manual-server", "1.0.0")
            .with_description("A manually implemented MCP server")
    }

    fn list_tools(&self) -> Vec<Tool> {
        let schema = ToolInputSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "message": { "type": "string" }
            })),
            required: Some(vec!["message".to_string()]),
            additional_properties: Some(false),
        };

        vec![
            Tool::new("echo", "Echo back the input")
                .with_schema(schema)
        ]
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
        args: Value,
        _ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<ToolResult>> + Send + 'a {
        let name = name.to_string();
        async move {
            match name.as_str() {
                "echo" => {
                    let msg = args.get("message")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| McpError::invalid_params("Missing 'message'"))?;
                    Ok(ToolResult::text(msg))
                }
                _ => Err(McpError::tool_not_found(&name))
            }
        }
    }

    fn read_resource<'a>(
        &'a self,
        uri: &'a str,
        _ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<ResourceResult>> + Send + 'a {
        let uri = uri.to_string();
        async move { Err(McpError::resource_not_found(&uri)) }
    }

    fn get_prompt<'a>(
        &'a self,
        name: &'a str,
        _args: Option<Value>,
        _ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<PromptResult>> + Send + 'a {
        let name = name.to_string();
        async move { Err(McpError::prompt_not_found(&name)) }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Run with STDIO transport
    ManualServer.run_stdio().await?;
    Ok(())
}
