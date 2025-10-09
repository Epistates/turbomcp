//! Stdio Transport Server - Minimal Example
//!
//! Demonstrates stdio transport for CLI tools and Claude Desktop integration.
//!
//! **Run standalone:**
//! ```bash
//! cargo run --example stdio_server --features stdio
//! ```
//!
//! **Test with JSON-RPC:**
//! ```bash
//! echo '{"jsonrpc":"2.0","method":"tools/list","id":1}' | cargo run --example stdio_server
//! ```

use turbomcp::prelude::*;

#[derive(Clone)]
struct StdioServer;

#[turbomcp::server(name = "stdio-demo", version = "1.0.0")]
impl StdioServer {
    #[tool("Echo a message")]
    async fn echo(&self, message: String) -> McpResult<String> {
        Ok(format!("Stdio Echo: {}", message))
    }

    #[tool("Reverse a string")]
    async fn reverse(&self, text: String) -> McpResult<String> {
        Ok(text.chars().rev().collect())
    }

    #[resource("stdio://status")]
    async fn status(&self) -> McpResult<String> {
        Ok("Stdio server running".to_string())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Logs MUST go to stderr for stdio transport
    tracing_subscriber::fmt()
        .with_env_filter("error")
        .with_writer(std::io::stderr)
        .init();

    StdioServer.run_stdio().await?;

    Ok(())
}
