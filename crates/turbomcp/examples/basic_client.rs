//! # Basic Client - Connecting and Calling Tools
//!
//! Demonstrates how to create a client that connects to an MCP server.
//!
//! Run server first: `cargo run --example hello_world`
//! Then run this: `cargo run --example basic_client`

use std::collections::HashMap;
use turbomcp_client::Client;
use turbomcp_transport::stdio::StdioTransport;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Logs must go to stderr for STDIO transport
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("🔌 Starting MCP client");

    // Create transport and client
    let transport = StdioTransport::new();
    let client = Client::new(transport);

    // Initialize connection
    let init_result = client.initialize().await?;
    tracing::info!("📋 Connected to: {}", init_result.server_info.name);

    // List available tools
    let tools = client.list_tools().await?;
    tracing::info!("🛠️  Found {} tools", tools.len());
    for tool in &tools {
        tracing::info!("  - {}", tool.name);
    }

    // Call a tool
    let mut args = HashMap::new();
    args.insert("name".to_string(), serde_json::json!("World"));
    let result = client.call_tool("hello", Some(args)).await?;
    tracing::info!("✅ Result: {:?}", result);

    tracing::info!("🔚 Client demo completed");
    Ok(())
}
