//! Stdio Transport Client - Minimal Example
//!
//! Demonstrates how to create a client that launches and connects to an MCP server via stdio.
//!
//! **Run:**
//! ```bash
//! cargo run --example stdio_client --features stdio
//! ```

use std::collections::HashMap;
use turbomcp_client::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Logs MUST go to stderr for stdio transport
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("🔌 Starting Stdio client");

    // Create stdio transport (connects to stdin/stdout of this process)
    // In production, you'd launch a child process and connect to its stdio
    let transport = StdioTransport::new();
    let client = Client::new(transport);

    // Initialize connection
    let init = client.initialize().await?;
    tracing::info!("✅ Connected to: {}", init.server_info.name);

    // List tools
    let tools = client.list_tools().await?;
    tracing::info!("🛠️  Found {} tools:", tools.len());
    for tool in &tools {
        tracing::info!(
            "  - {}: {}",
            tool.name,
            tool.description.as_deref().unwrap_or("")
        );
    }

    // Call echo tool
    let mut args = HashMap::new();
    args.insert("message".to_string(), serde_json::json!("Hello Stdio!"));
    let result = client.call_tool("echo", Some(args)).await?;
    tracing::info!("📝 Echo result: {:?}", result);

    // Call reverse tool
    let mut args = HashMap::new();
    args.insert("text".to_string(), serde_json::json!("TurboMCP"));
    let result = client.call_tool("reverse", Some(args)).await?;
    tracing::info!("🔄 Reverse result: {:?}", result);

    tracing::info!("✅ Demo complete");
    Ok(())
}
