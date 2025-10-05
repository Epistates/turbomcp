//! STDIO Transport Client
//!
//! This example demonstrates how to create a client that connects to
//! a STDIO-based MCP server.
//!
//! Run with: `cargo run --example transport_stdio_client`

use std::collections::HashMap;
use std::process::Stdio;
use tokio::process::Command;
use turbomcp_client::Client;
use turbomcp_transport::stdio::StdioTransport;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // CRITICAL: For MCP STDIO protocol, logs MUST go to stderr, not stdout
    // stdout is reserved for pure JSON-RPC messages only
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr) // Fix: Send logs to stderr
        .init();

    tracing::info!("🔌 Starting STDIO Transport Client");

    // Start the calculator server as a child process
    let mut server_process = Command::new("cargo")
        .args(["run", "--example", "transport_stdio_server"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;

    // Create STDIO transport and client
    let transport = StdioTransport::new();
    let client = Client::new(transport);

    tracing::info!("✅ Connected to STDIO server");

    // Initialize the connection
    let init_result = client.initialize().await?;
    tracing::info!("📋 Server: {}", init_result.server_info.name);
    tracing::info!("🔧 Version: {}", init_result.server_info.version);

    // List available tools
    let tools = client.list_tools().await?;
    tracing::info!("🛠️  Available tools: {}", tools.len());
    for tool in &tools {
        tracing::info!(
            "  - {} - {}",
            tool.name,
            tool.description.as_deref().unwrap_or("No description")
        );
    }

    // Call some tools
    tracing::info!("🧮 Testing calculator operations...");

    let mut args = HashMap::new();
    args.insert("a".to_string(), serde_json::json!(10.0));
    args.insert("b".to_string(), serde_json::json!(5.0));
    let result = client.call_tool("add", Some(args.clone())).await?;
    tracing::info!("➕ 10 + 5 = {:?}", result);

    args.clear();
    args.insert("a".to_string(), serde_json::json!(7.0));
    args.insert("b".to_string(), serde_json::json!(6.0));
    let result = client.call_tool("multiply", Some(args.clone())).await?;
    tracing::info!("✖️  7 × 6 = {:?}", result);

    args.clear();
    args.insert("a".to_string(), serde_json::json!(20.0));
    args.insert("b".to_string(), serde_json::json!(4.0));
    let result = client.call_tool("divide", Some(args.clone())).await?;
    tracing::info!("➗ 20 ÷ 4 = {:?}", result);

    // Test resource access
    let resources = client.list_resources().await?;
    if !resources.is_empty() {
        tracing::info!("📁 Available resources: {}", resources.len());
        for resource_uri in &resources {
            let content = client.read_resource(resource_uri).await?;
            tracing::info!("📄 {}: {:?}", resource_uri, content.contents);
        }
    }

    // Clean shutdown
    server_process.kill().await?;
    tracing::info!("🔚 STDIO client demo completed");

    Ok(())
}
