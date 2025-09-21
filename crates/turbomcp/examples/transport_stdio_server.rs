//! STDIO Transport Server - Standard MCP Protocol
//!
//! This example demonstrates the STDIO transport which is the standard
//! MCP protocol transport used by Claude Desktop and most MCP clients.
//!
//! Run with: `cargo run --example transport_stdio_server`

use turbomcp::prelude::*;

/// Simple calculation server using STDIO transport (macro approach)
#[derive(Clone)]
struct CalculatorServer;

#[server(
    name = "Calculator Server",
    version = "1.0.0",
    description = "Math operations via STDIO transport"
)]
impl CalculatorServer {
    #[tool("Add two numbers")]
    async fn add(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a + b)
    }

    #[tool("Subtract two numbers")]
    async fn subtract(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a - b)
    }

    #[tool("Multiply two numbers")]
    async fn multiply(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a * b)
    }

    #[tool("Divide two numbers")]
    async fn divide(&self, a: f64, b: f64) -> McpResult<f64> {
        if b == 0.0 {
            return Err(McpError::tool("Division by zero is not allowed"));
        }
        Ok(a / b)
    }

    #[resource("file:///calculator/history")]
    async fn get_history(&self, _ctx: Context) -> McpResult<String> {
        Ok(
            "Calculator operation history:\n- Add, Subtract, Multiply, Divide operations available"
                .to_string(),
        )
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // CRITICAL: For MCP STDIO protocol, do NOT initialize any logging
    // stdout is reserved exclusively for JSON-RPC messages
    // stderr should also be avoided as it may interfere with some clients
    // Any output will break the MCP protocol communication

    let server = CalculatorServer;

    // STDIO transport - the standard MCP protocol
    server.run_stdio().await?;

    Ok(())
}
