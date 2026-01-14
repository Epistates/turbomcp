//! v3 Pristine Architecture Example - Calculator Server
//!
//! This example demonstrates the v3 architecture with zero-boilerplate macros.
//!
//! Features demonstrated:
//! - `#[server]` macro generates `McpHandler` trait implementation
//! - `#[tool]` attributes extract schemas from function signatures
//! - Transport-agnostic design (same code works on WASM and native)
//!
//! # Running
//!
//! ```bash
//! cargo run --example v3_calculator --features stdio
//! ```
//!
//! # Testing with CLI
//!
//! ```bash
//! # Initialize (MCP spec requires clientInfo)
//! echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","clientInfo":{"name":"cli","version":"1.0"},"capabilities":{}}}' | cargo run --example v3_calculator --features stdio
//! echo '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | cargo run --example v3_calculator --features stdio
//! echo '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"add","arguments":{"a":5,"b":3}}}' | cargo run --example v3_calculator --features stdio
//! ```

use turbomcp::prelude::*;

/// A simple calculator server demonstrating v3 pristine architecture.
#[derive(Clone)]
struct Calculator;

#[server(
    name = "v3-calculator",
    version = "1.0.0",
    description = "A pristine v3 calculator"
)]
impl Calculator {
    /// Add two numbers together.
    #[tool]
    async fn add(&self, a: i64, b: i64) -> i64 {
        a + b
    }

    /// Subtract b from a.
    #[tool]
    async fn subtract(&self, a: i64, b: i64) -> i64 {
        a - b
    }

    /// Multiply two numbers.
    #[tool]
    async fn multiply(&self, a: i64, b: i64) -> i64 {
        a * b
    }

    /// Greet someone by name.
    #[tool]
    async fn greet(&self, name: String) -> String {
        format!("Hello, {}!", name)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for logging
    tracing_subscriber::fmt::init();

    tracing::info!("Starting v3 Calculator Server...");

    // Run the server on STDIO transport
    // The #[server] macro generates the McpHandler implementation
    // which provides run_stdio(), run_http(), run_tcp(), etc.
    Calculator.run_stdio().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp_server::v3::RequestContext;

    #[test]
    fn test_server_info() {
        let calc = Calculator;
        let info = calc.server_info();
        assert_eq!(info.name, "v3-calculator");
        assert_eq!(info.version, "1.0.0");
    }

    #[test]
    fn test_list_tools() {
        let calc = Calculator;
        let tools = calc.list_tools();
        assert_eq!(tools.len(), 4); // add, subtract, multiply, greet
        assert!(tools.iter().any(|t| t.name == "add"));
        assert!(tools.iter().any(|t| t.name == "greet"));
    }

    #[tokio::test]
    async fn test_add() {
        let calc = Calculator;
        let ctx = RequestContext::new();
        let result = calc
            .call_tool("add", serde_json::json!({"a": 10, "b": 20}), &ctx)
            .await
            .unwrap();
        // The result should contain "30" as text
        assert!(result.first_text().unwrap().contains("30"));
    }

    #[tokio::test]
    async fn test_handle_request() {
        let calc = Calculator;
        let ctx = RequestContext::new();

        // Test initialize (MCP spec requires clientInfo)
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "clientInfo": {
                    "name": "test-client",
                    "version": "1.0.0"
                },
                "capabilities": {}
            }
        });
        let response = calc.handle_request(request, ctx.clone()).await.unwrap();
        assert_eq!(response["result"]["serverInfo"]["name"], "v3-calculator");
        // Verify MCP-compliant capability structure
        assert!(
            response["result"]["capabilities"]["tools"]["listChanged"]
                .as_bool()
                .unwrap_or(false)
        );

        // Test tools/call
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "multiply",
                "arguments": {"a": 6, "b": 7}
            }
        });
        let response = calc.handle_request(request, ctx).await.unwrap();
        assert!(response.get("error").is_none());
    }
}
