//! TurboMCP Demo - v3 Architecture
//!
//! This demo showcases the v3 zero-boilerplate MCP server API.

use turbomcp::prelude::*;

#[derive(Clone)]
struct DemoServer;

#[server(name = "turbomcp-demo", version = "3.0.0")]
impl DemoServer {
    /// Say hello to someone
    #[tool]
    async fn hello(&self, name: Option<String>) -> String {
        let name = name.unwrap_or_else(|| "World".to_string());
        format!("Hello, {name}! Welcome to TurboMCP!")
    }

    /// Add two numbers together
    #[tool]
    async fn add(&self, a: i64, b: i64) -> i64 {
        a + b
    }

    /// Get the current time
    #[tool]
    async fn current_time(&self) -> String {
        chrono::Utc::now().to_rfc3339()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Run with STDIO - the simplest possible MCP server
    DemoServer.run().await?;
    Ok(())
}
