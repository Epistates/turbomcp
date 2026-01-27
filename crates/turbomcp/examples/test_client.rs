//! # Test Client Example
//!
//! Demonstrates using McpTestClient for testing MCP servers without
//! network transport overhead.
//!
//! This enables:
//! - Fast unit tests (no TCP/HTTP setup)
//! - Fluent assertion API
//! - Session simulation
//!
//! Run with: `cargo run --example test_client`

use turbomcp::prelude::*;
use turbomcp::testing::{McpTestClient, ToolResultAssertions};

// ============================================================================
// Calculator Server (to be tested)
// ============================================================================

#[derive(Clone)]
struct Calculator;

#[turbomcp::server(name = "calculator", version = "1.0.0")]
impl Calculator {
    /// Add two numbers
    #[tool(description = "Add two numbers")]
    async fn add(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a + b)
    }

    /// Divide two numbers
    #[tool(description = "Divide two numbers")]
    async fn divide(&self, a: f64, b: f64) -> McpResult<f64> {
        if b == 0.0 {
            return Err(McpError::invalid_params("Division by zero"));
        }
        Ok(a / b)
    }

    /// Get calculator info
    #[resource("info://calculator")]
    async fn get_info(&self, _uri: String, _ctx: &RequestContext) -> McpResult<String> {
        Ok(r#"{"name": "Calculator", "version": "1.0"}"#.into())
    }

    /// Help prompt
    #[prompt(description = "How to use the calculator")]
    async fn help(&self, ctx: &RequestContext) -> McpResult<PromptResult> {
        let _ = ctx;
        Ok(PromptResult::user(
            "Use add(a, b) to add numbers and divide(a, b) to divide.",
        ))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== McpTestClient Demo ===\n");

    tokio::runtime::Runtime::new()?.block_on(async {
        // Create test client wrapping our server
        let client = McpTestClient::new(Calculator);

        // =====================================================================
        // Basic Tool Calls
        // =====================================================================
        println!("Basic tool calls:");
        println!("-----------------");

        // Call a tool with arguments
        let result = client.call_tool("add", serde_json::json!({"a": 5.0, "b": 3.0})).await?;
        println!("add(5, 3) = {:?}", result.first_text());

        // Call with different values
        let result = client.call_tool("divide", serde_json::json!({"a": 10.0, "b": 2.0})).await?;
        println!("divide(10, 2) = {:?}", result.first_text());
        println!();

        // =====================================================================
        // Fluent Assertions
        // =====================================================================
        println!("Fluent assertions:");
        println!("------------------");

        // Assert on successful result
        let result = client.call_tool("add", serde_json::json!({"a": 2.0, "b": 2.0})).await?;
        result.assert_text("4");
        println!("✓ add(2, 2) = 4");

        // Assert text contains
        let result = client.call_tool("add", serde_json::json!({"a": 100.0, "b": 0.5})).await?;
        result.assert_text_contains("100");
        println!("✓ add(100, 0.5) contains '100'");

        // Assert error on invalid input
        let result = client.call_tool("divide", serde_json::json!({"a": 1.0, "b": 0.0})).await;
        assert!(result.is_err(), "Expected error for division by zero");
        println!("✓ divide(1, 0) returns error");
        println!();

        // =====================================================================
        // Listing Tools, Resources, Prompts
        // =====================================================================
        println!("Listing components:");
        println!("-------------------");

        let tools = client.list_tools();
        println!("Tools: {:?}", tools.iter().map(|t| &t.name).collect::<Vec<_>>());

        let resources = client.list_resources();
        println!("Resources: {:?}", resources.iter().map(|r| &r.uri).collect::<Vec<_>>());

        let prompts = client.list_prompts();
        println!("Prompts: {:?}", prompts.iter().map(|p| &p.name).collect::<Vec<_>>());
        println!();

        // =====================================================================
        // Existence Assertions
        // =====================================================================
        println!("Existence assertions:");
        println!("---------------------");

        client.assert_tool_exists("add");
        println!("✓ Tool 'add' exists");

        client.assert_resource_exists("info://calculator");
        println!("✓ Resource 'info://calculator' exists");

        client.assert_prompt_exists("help");
        println!("✓ Prompt 'help' exists");
        println!();

        // =====================================================================
        // Reading Resources
        // =====================================================================
        println!("Reading resources:");
        println!("------------------");

        let result = client.read_resource("info://calculator").await?;
        let text = result.contents.first()
            .and_then(|c| c.text.as_ref())
            .map(|s| s.as_str())
            .unwrap_or("");
        println!("info://calculator = {}", text);
        println!();

        // =====================================================================
        // Getting Prompts
        // =====================================================================
        println!("Getting prompts:");
        println!("----------------");

        let result = client.get_prompt_empty("help").await?;
        println!("help prompt = {:?}", result.messages.first().map(|m| &m.content));
        println!();

        // =====================================================================
        // Session Support
        // =====================================================================
        println!("Session support:");
        println!("----------------");

        let client_with_session = client.with_session("user-123");
        // The session ID is now attached to all requests
        let result = client_with_session.call_tool("add", serde_json::json!({"a": 1.0, "b": 1.0})).await?;
        println!("With session 'user-123': add(1, 1) = {:?}", result.first_text());
        println!();

        // =====================================================================
        // Testing Pattern Summary
        // =====================================================================
        println!("=== Testing Pattern Summary ===\n");
        println!("In your tests, use:");
        println!();
        println!("  #[tokio::test]");
        println!("  async fn test_calculator() {{");
        println!("      let client = McpTestClient::new(Calculator);");
        println!();
        println!("      // Test successful call");
        println!("      let result = client.call_tool(\"add\", json!({{\"a\": 2, \"b\": 3}})).await.unwrap();");
        println!("      result.assert_text(\"5\");");
        println!();
        println!("      // Test error case");
        println!("      let result = client.call_tool(\"divide\", json!({{\"a\": 1, \"b\": 0}})).await;");
        println!("      assert!(result.is_err());");
        println!("  }}");

        Ok::<_, McpError>(())
    })?;

    Ok(())
}
