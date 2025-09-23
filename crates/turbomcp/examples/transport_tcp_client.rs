//! TCP Transport Client
//!
//! This example demonstrates how to create a client that connects to
//! a TCP-based MCP server for high-performance communication.
//!
//! Run with: `cargo run --example transport_tcp_client`

use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::time::{Duration, sleep};
use turbomcp_client::Client;
use turbomcp_transport::{Transport, tcp::TcpTransport};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // CRITICAL: For MCP STDIO protocol, logs MUST go to stderr, not stdout
    // stdout is reserved for pure JSON-RPC messages only
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr) // Fix: Send logs to stderr
        .init();

    tracing::info!("📁 Starting TCP Transport Client");

    // Wait for server to be ready (if running manually)
    tracing::info!("⏳ Connecting to TCP server at 127.0.0.1:7071...");
    sleep(Duration::from_millis(1000)).await;

    // Create TCP transport and client
    let bind_addr: SocketAddr = "0.0.0.0:0".parse()?; // Auto-assign local port
    let remote_addr: SocketAddr = "127.0.0.1:7071".parse()?;
    let mut transport = TcpTransport::new_client(bind_addr, remote_addr);
    transport.connect().await?;
    let mut client = Client::new(transport);

    tracing::info!("✅ Connected to TCP server");

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

    // Test file operations
    tracing::info!("📁 Testing high-performance file operations...");

    // List existing files
    let mut args = HashMap::new();
    let result = client.call_tool("list_files", Some(args.clone())).await?;
    tracing::info!("📋 {}", result);

    // Read an existing file
    args.clear();
    args.insert("filename".to_string(), serde_json::json!("readme.txt"));
    let result = client.call_tool("read_file", Some(args.clone())).await?;
    tracing::info!("📖 {}", result);

    // Create a new file
    args.clear();
    args.insert("filename".to_string(), serde_json::json!("tcp_test.txt"));
    args.insert("content".to_string(), serde_json::json!("This file was created via TCP transport!\nHigh-performance communication is working perfectly.\n\nTCP benefits:\n- Low latency\n- Direct socket communication\n- Efficient for internal services"));
    let result = client.call_tool("write_file", Some(args.clone())).await?;
    tracing::info!("✍️  {}", result);

    // Get file statistics
    args.clear();
    args.insert("filename".to_string(), serde_json::json!("tcp_test.txt"));
    let result = client.call_tool("get_stats", Some(args.clone())).await?;
    tracing::info!("📊 {}", result);

    // Read the new file
    let result = client.call_tool("read_file", Some(args.clone())).await?;
    tracing::info!("📖 {}", result);

    // List files again to see the new one
    args.clear();
    let result = client.call_tool("list_files", Some(args.clone())).await?;
    tracing::info!("📋 {}", result);

    // Create another file for testing
    args.clear();
    args.insert("filename".to_string(), serde_json::json!("performance.log"));
    args.insert("content".to_string(), serde_json::json!("TCP Performance Test Results:\nLatency: Low\nThroughput: High\nReliability: Excellent\nUse case: Internal services, high-frequency operations"));
    let result = client.call_tool("write_file", Some(args.clone())).await?;
    tracing::info!("✍️  {}", result);

    // Test resource access
    let resources = client.list_resources().await?;
    tracing::info!("📁 Available resources: {}", resources.len());
    for resource_uri in &resources {
        let content = client.read_resource(resource_uri).await?;
        tracing::info!("📄 {}:", resource_uri);
        for content_item in &content.contents {
            tracing::info!("  {:?}", content_item);
        }
    }

    // Clean up test files
    args.clear();
    args.insert("filename".to_string(), serde_json::json!("tcp_test.txt"));
    let result = client.call_tool("delete_file", Some(args.clone())).await?;
    tracing::info!("🗑️  {}", result);

    args.clear();
    args.insert("filename".to_string(), serde_json::json!("performance.log"));
    let result = client.call_tool("delete_file", Some(args.clone())).await?;
    tracing::info!("🗑️  {}", result);

    // Final file list
    args.clear();
    let result = client.call_tool("list_files", Some(args.clone())).await?;
    tracing::info!("📋 Final: {}", result);

    tracing::info!("🔚 TCP client demo completed");
    tracing::info!("💡 TCP transport provides excellent performance for internal services!");

    Ok(())
}
