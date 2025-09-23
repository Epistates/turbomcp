//! Clean TurboMCP Demo - JSON-RPC ONLY output
//!
//! This demo outputs ONLY JSON-RPC messages for MCP STDIO transport compliance.
//! No logging, no banners, no extra output - pure protocol communication.

use std::collections::HashMap;
use turbomcp_protocol::types::{
    CallToolRequest, CallToolResult, Content, TextContent, Tool, ToolInputSchema,
};
use turbomcp_server::{handlers::FunctionToolHandler, ServerBuilder};

/// Simple hello function for MCP testing
async fn hello(
    req: CallToolRequest,
    _ctx: turbomcp_core::RequestContext,
) -> Result<CallToolResult, turbomcp_server::ServerError> {
    let name = req
        .arguments
        .as_ref()
        .and_then(|args| args.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("World");

    let greeting = format!("Hello, {name}! Welcome to TurboMCP! 🦀⚡");

    Ok(CallToolResult {
        content: vec![Content::Text(TextContent {
            text: greeting,
            annotations: None,
            meta: None,
        })],
        is_error: None,
        structured_content: None,
        _meta: None,
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // CRITICAL: NO LOGGING - stdout reserved for JSON-RPC only
    // TEST COMMENT TO FORCE REBUILD

    // Create minimal tool schema
    let tool = Tool {
        name: "hello".to_string(),
        title: Some("Hello".to_string()),
        description: Some("Say hello to someone".to_string()),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: Some({
                let mut props = HashMap::new();
                props.insert(
                    "name".to_string(),
                    serde_json::json!({
                        "type": "string",
                        "description": "The name to greet"
                    }),
                );
                props
            }),
            required: None,
            additional_properties: Some(false),
        },
        output_schema: None,
        annotations: None,
        meta: None,
    };

    // Build minimal server - STDIO compliant
    let server = ServerBuilder::new()
        .name("TurboMCP-Demo")
        .version("1.0.8")
        .description("Clean MCP demo - JSON only")
        .tool("hello", FunctionToolHandler::new(tool, hello))?
        .build();

    // Run with STDIO - NO logging output
    server.run_stdio().await?;

    Ok(())
}
