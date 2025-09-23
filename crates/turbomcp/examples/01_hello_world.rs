#![allow(dead_code)]
//! # 01: Hello World - Your First TurboMCP Server
//!
//! **Learning Goals (5 minutes):**
//! - Create the simplest possible working MCP server with proper tool schemas
//! - Understand basic server setup and execution
//! - See how MCP protocol communication works
//!
//! **What this example demonstrates:**
//! - Minimal server configuration with proper tool schema
//! - Tool registration with complete parameter schemas
//! - JSON-RPC communication over stdio
//!
//! **Run with:** `cargo run --example 01_hello_world`
//! **Test with Claude Desktop** by adding to your MCP configuration

use std::collections::HashMap;
use turbomcp_protocol::types::{
    CallToolRequest, CallToolResult, Content, TextContent, Tool, ToolInputSchema,
};
use turbomcp_server::{ServerBuilder, handlers::FunctionToolHandler};

/// Simple hello function that will be our tool handler
async fn hello(
    req: CallToolRequest,
    _ctx: turbomcp_core::RequestContext,
) -> Result<CallToolResult, turbomcp_server::ServerError> {
    // Extract the name parameter from the request
    let name = req
        .arguments
        .as_ref()
        .and_then(|args| args.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("World");

    let greeting = format!("Hello, {name}! Welcome to TurboMCP! 🦀⚡");

    // Return the greeting as a text content result
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
    // CRITICAL: For MCP STDIO protocol, do NOT initialize any logging
    // stdout is reserved exclusively for JSON-RPC messages
    // stderr should also be avoided as it may interfere with some clients
    // If logging is absolutely needed for debugging, use RUST_LOG=debug and stderr only

    // Create tool with complete schema
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
            required: None, // name is optional, defaults to "World"
            additional_properties: Some(false),
        },
        output_schema: None,
        annotations: None,
        meta: None,
    };

    // Create handler
    let handler = FunctionToolHandler::new(tool, hello);

    // Build the server with tool registration
    let server = ServerBuilder::new()
        .name("HelloWorld")
        .version("1.0.0")
        .description("A simple hello world MCP server with complete tool schemas")
        .tool("hello", handler)?
        .build();

    // Run the server with STDIO transport
    server.run_stdio().await?;

    Ok(())
}

// 🎯 **Try it out:**
//
//    Run the server:
//    cargo run --example 01_hello_world
//
//    Then connect with Claude Desktop or test with JSON-RPC:
//    - Tool: hello
//    - Parameters: { "name": "Alice" }
//    - Response: "Hello, Alice! Welcome to TurboMCP! 🦀⚡"

/* 📝 **Key Concepts:**

**MCP Server Structure:**
- ServerBuilder creates the server configuration
- Tools have complete schemas with parameter definitions
- FunctionToolHandler provides type-safe tool registration

**Tool Schema Definition:**
- Complete Tool struct with input_schema
- JSON Schema properties for parameters
- Optional vs required parameter handling
- Proper type definitions and descriptions

**Tool Handler Pattern:**
- Receives CallToolRequest with arguments
- Processes the request and generates a response
- Returns CallToolResult with content

**Transport Layer:**
- run_stdio() uses standard input/output for communication
- JSON-RPC protocol over stdio
- Perfect for integration with Claude Desktop

**Benefits:**
- AI models see proper parameter schemas
- Optional parameters work correctly
- Better developer experience
- No mock or placeholder code

**Next Steps:**
- Add more tools with complex schemas
- Try optional and required parameters
- Explore the macro approach for even easier development

**Next Example:** `02_tools_basics.rs` - Essential tool patterns and validation
*/
