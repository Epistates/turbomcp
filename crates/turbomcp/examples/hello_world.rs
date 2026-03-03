//! # Hello World Server
//!
//! The absolute simplest MCP server - one tool, minimal code.
//!
//! Run with: `cargo run --example hello_world`

use turbomcp::prelude::*;

#[derive(Clone)]
struct HelloServer;

#[turbomcp::server(name = "hello", version = "1.0.0")]
impl HelloServer {
    /// Say hello to someone
    #[tool(description = "Say hello to someone")]
    async fn hello(&self, name: String) -> McpResult<String> {
        Ok(format!("Hello, {}!", name))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing to stderr (MUST NOT write to stdout as it pollutes the MCP protocol)
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tracing::info!("Starting Hello World Server...");

    HelloServer.run_stdio().await?;
    Ok(())
}
