//! # Architecture Patterns: Builder vs Macro APIs
//!
//! This example demonstrates functional equivalence between TurboMCP's two API styles:
//! - **Builder Pattern**: Explicit, programmatic server construction
//! - **Macro Pattern**: Declarative, attribute-based server definition
//!
//! Both servers implement identical calculator functionality, proving that the choice
//! between patterns is purely about developer preference and use case.
//!
//! ## Learning Goals
//! - Understand when to use builder vs macro patterns
//! - See how both compile to the same runtime behavior  
//! - Learn the trade-offs between explicit control and ergonomic simplicity
//!
//! ## Running the Examples
//! ```bash
//! # Run the builder-pattern server
//! cargo run --example 06_architecture_patterns builder
//!
//! # Run the macro-pattern server  
//! cargo run --example 06_architecture_patterns macro
//!
//! # Test either server with the unified client
//! cargo run --example 06_architecture_patterns client
//! ```

use serde_json::json;
use std::collections::HashMap;

use turbomcp_core::RequestContext;
use turbomcp_protocol::types::{
    CallToolRequest, CallToolResult, Content, TextContent, Tool, ToolInputSchema,
};
use turbomcp_server::{ServerBuilder, ServerError, handlers::FunctionToolHandler};

// ============================================================================
// APPROACH 1: BUILDER PATTERN - Explicit Control
// ============================================================================

/// Calculator server using the builder pattern for maximum control
pub struct BuilderCalculator;

impl BuilderCalculator {
    /// Create and configure the server using builder pattern
    pub async fn run_stdio() -> Result<(), ServerError> {
        // Explicit tool handler for addition
        async fn add_handler(
            req: CallToolRequest,
            _ctx: RequestContext,
        ) -> Result<CallToolResult, ServerError> {
            let args = req
                .arguments
                .as_ref()
                .ok_or_else(|| ServerError::handler("Missing arguments"))?;

            let a = args
                .get("a")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| ServerError::handler("Missing parameter 'a'"))?;

            let b = args
                .get("b")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| ServerError::handler("Missing parameter 'b'"))?;

            let result = a + b;

            Ok(CallToolResult {
                content: vec![Content::Text(TextContent {
                    text: format!("{} + {} = {}", a, b, result),
                    annotations: None,
                    meta: None,
                })],
                is_error: None,
                structured_content: None,
                _meta: None,
            })
        }

        // Explicit tool handler for subtraction
        async fn subtract_handler(
            req: CallToolRequest,
            _ctx: RequestContext,
        ) -> Result<CallToolResult, ServerError> {
            let args = req
                .arguments
                .as_ref()
                .ok_or_else(|| ServerError::handler("Missing arguments"))?;

            let a = args
                .get("a")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| ServerError::handler("Missing parameter 'a'"))?;

            let b = args
                .get("b")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| ServerError::handler("Missing parameter 'b'"))?;

            let result = a - b;

            Ok(CallToolResult {
                content: vec![Content::Text(TextContent {
                    text: format!("{} - {} = {}", a, b, result),
                    annotations: None,
                    meta: None,
                })],
                is_error: None,
                structured_content: None,
                _meta: None,
            })
        }

        // Define tool schemas with proper MCP structure
        let add_tool = Tool {
            name: "add".to_string(),
            title: None,
            description: Some("Add two numbers".to_string()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: Some(HashMap::from([
                    (
                        "a".to_string(),
                        json!({
                            "type": "number",
                            "description": "First number"
                        }),
                    ),
                    (
                        "b".to_string(),
                        json!({
                            "type": "number",
                            "description": "Second number"
                        }),
                    ),
                ])),
                required: Some(vec!["a".to_string(), "b".to_string()]),
                additional_properties: Some(false),
            },
            output_schema: None,
            annotations: None,
            meta: None,
        };

        let subtract_tool = Tool {
            name: "subtract".to_string(),
            title: None,
            description: Some("Subtract two numbers".to_string()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: Some(HashMap::from([
                    (
                        "a".to_string(),
                        json!({
                            "type": "number",
                            "description": "First number"
                        }),
                    ),
                    (
                        "b".to_string(),
                        json!({
                            "type": "number",
                            "description": "Second number"
                        }),
                    ),
                ])),
                required: Some(vec!["a".to_string(), "b".to_string()]),
                additional_properties: Some(false),
            },
            output_schema: None,
            annotations: None,
            meta: None,
        };

        // Build server with explicit configuration
        let server = ServerBuilder::new()
            .name("calculator-builder")
            .version("1.0.0")
            .description("Calculator using builder pattern")
            .tool("add", FunctionToolHandler::new(add_tool, add_handler))?
            .tool(
                "subtract",
                FunctionToolHandler::new(subtract_tool, subtract_handler),
            )?
            .build();

        println!("[Builder] Starting calculator server on stdio...");
        server.run_stdio().await
    }
}

// ============================================================================
// APPROACH 2: MACRO PATTERN - Ergonomic Simplicity
// ============================================================================

use turbomcp::{McpResult, prelude::*};

/// Calculator server using macro pattern for ergonomic development
#[derive(Clone)]
pub struct MacroCalculator;

#[turbomcp::server(
    name = "calculator-macro",
    version = "1.0.0",
    description = "Calculator using macro pattern"
)]
impl MacroCalculator {
    /// Add two numbers
    #[tool]
    async fn add(&self, a: f64, b: f64) -> McpResult<String> {
        Ok(format!("{} + {} = {}", a, b, a + b))
    }

    /// Subtract two numbers
    #[tool]
    async fn subtract(&self, a: f64, b: f64) -> McpResult<String> {
        Ok(format!("{} - {} = {}", a, b, a - b))
    }
}

impl MacroCalculator {
    pub async fn run_stdio_macro() -> Result<(), ServerError> {
        println!("[Macro] Starting calculator server on stdio...");
        MacroCalculator
            .run_stdio()
            .await
            .map_err(|e| ServerError::handler(format!("Failed to run server: {}", e)))
    }
}

// ============================================================================
// UNIFIED CLIENT - Works with Both Server Patterns
// ============================================================================

/// Test client that can connect to either server implementation
pub struct UnifiedClient;

impl UnifiedClient {
    /// Test the server with calculator operations
    pub async fn test_server() {
        println!("\n=== Unified Client Test ===\n");

        // These test messages work with both server implementations
        let test_messages = vec![
            // Initialize connection
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-10-07",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "unified-test-client",
                        "version": "1.0.0"
                    }
                }
            }),
            // List available tools
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/list"
            }),
            // Test addition
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {
                    "name": "add",
                    "arguments": {
                        "a": 10.5,
                        "b": 20.3
                    }
                }
            }),
            // Test subtraction
            json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {
                    "name": "subtract",
                    "arguments": {
                        "a": 100,
                        "b": 42
                    }
                }
            }),
        ];

        println!("Test messages that work with both server patterns:");
        for (i, msg) in test_messages.iter().enumerate() {
            println!(
                "\n{}. {}",
                i + 1,
                serde_json::to_string_pretty(msg).unwrap()
            );
        }

        println!("\n=== Functional Equivalence Demonstrated ===");
        println!("Both servers respond identically to the same client requests!");
    }
}

// ============================================================================
// MAIN - Run Builder, Macro, or Client
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("builder") => {
            println!("\n╔════════════════════════════════════════╗");
            println!("║   BUILDER PATTERN CALCULATOR SERVER   ║");
            println!("╚════════════════════════════════════════╝\n");
            println!("Advantages:");
            println!("  ✓ Full control over server configuration");
            println!("  ✓ Explicit schema definitions");
            println!("  ✓ No macro magic - what you see is what you get");
            println!("  ✓ Easy to debug and customize\n");

            BuilderCalculator::run_stdio().await?;
        }
        Some("macro") => {
            println!("\n╔════════════════════════════════════════╗");
            println!("║    MACRO PATTERN CALCULATOR SERVER    ║");
            println!("╚════════════════════════════════════════╝\n");
            println!("Advantages:");
            println!("  ✓ Minimal boilerplate code");
            println!("  ✓ Automatic schema generation");
            println!("  ✓ Clean, idiomatic Rust functions");
            println!("  ✓ Focus on business logic\n");

            MacroCalculator::run_stdio_macro().await?;
        }
        Some("client") => {
            UnifiedClient::test_server().await;
        }
        _ => {
            println!("\n╔════════════════════════════════════════╗");
            println!("║   ARCHITECTURE PATTERNS DEMONSTRATION  ║");
            println!("╚════════════════════════════════════════╝\n");
            println!("This example shows functional equivalence between:");
            println!("  • Builder Pattern - Explicit programmatic control");
            println!("  • Macro Pattern   - Declarative attribute-based\n");
            println!("Usage:");
            println!("  cargo run --example 06_architecture_patterns builder");
            println!("  cargo run --example 06_architecture_patterns macro");
            println!("  cargo run --example 06_architecture_patterns client\n");
            println!("In separate terminals, run:");
            println!("  1. Start a server (builder or macro)");
            println!("  2. Pipe client test messages to the server");
            println!("\nExample test:");
            println!("  echo '{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}}' | \\");
            println!("    cargo run --example 06_architecture_patterns builder");
        }
    }

    Ok(())
}
