//! Unix Socket Transport Client
//!
//! This example demonstrates how to create a client that connects to
//! a Unix socket-based MCP server for local inter-process communication.
//!
//! Run with: `cargo run --example transport_unix_client`

use std::collections::HashMap;
use std::path::PathBuf;
use tokio::time::{Duration, sleep};
use turbomcp_client::Client;
use turbomcp_transport::{Transport, unix::UnixTransport};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // CRITICAL: For MCP STDIO protocol, logs MUST go to stderr, not stdout
    // stdout is reserved for pure JSON-RPC messages only
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr) // Fix: Send logs to stderr
        .init();

    tracing::info!("🔄 Starting Unix Socket Transport Client");

    // Wait for server to be ready (if running manually)
    tracing::info!("⏳ Connecting to Unix socket at /tmp/turbomcp-process.sock...");
    sleep(Duration::from_millis(1000)).await;

    // Create Unix socket transport and client
    let socket_path = PathBuf::from("/tmp/turbomcp-process.sock");
    let transport = UnixTransport::new_client(socket_path);
    transport.connect().await?;
    let client = Client::new(transport);

    tracing::info!("✅ Connected to Unix socket server");

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

    // Test process management operations
    tracing::info!("🔄 Testing local process management...");

    // Get system statistics
    let args = HashMap::new();
    let result = client.call_tool("get_system_stats", Some(args)).await?;
    tracing::info!("📊 {}", result);

    // List all processes
    let args = HashMap::new();
    let result = client.call_tool("list_processes", Some(args)).await?;
    tracing::info!("📋 {}", result);

    // Start a new process
    let mut args = HashMap::new();
    args.insert("name".to_string(), serde_json::json!("unix-test-daemon"));
    let result = client.call_tool("start_process", Some(args)).await?;
    tracing::info!("🚀 {}", result);

    // Start another process
    let mut args = HashMap::new();
    args.insert("name".to_string(), serde_json::json!("local-ipc-service"));
    let result = client.call_tool("start_process", Some(args)).await?;
    tracing::info!("🚀 {}", result);

    // List processes again to see the new ones
    let args = HashMap::new();
    let result = client.call_tool("list_processes", Some(args)).await?;
    tracing::info!("📋 {}", result);

    // Get details of a specific process
    let mut args = HashMap::new();
    args.insert("pid".to_string(), serde_json::json!(1001));
    let result = client.call_tool("get_process", Some(args)).await?;
    tracing::info!("🔍 {}", result);

    // Get details of a newly created process
    let mut args = HashMap::new();
    args.insert("pid".to_string(), serde_json::json!(1004));
    let result = client.call_tool("get_process", Some(args.clone())).await?;
    tracing::info!("🔍 {}", result);

    // Stop a process
    let result = client.call_tool("stop_process", Some(args)).await?;
    tracing::info!("🛑 {}", result);

    // Get updated system stats
    let args = HashMap::new();
    let result = client.call_tool("get_system_stats", Some(args)).await?;
    tracing::info!("📊 {}", result);

    // Test resource access
    let resources = client.list_resources().await?;
    tracing::info!("📁 Available resources: {}", resources.len());
    for resource_uri in &resources {
        let content = client.read_resource(resource_uri).await?;
        tracing::info!("📄 {}:\n{:?}", resource_uri, content.contents);
    }

    // Try to get a non-existent process
    let mut args = HashMap::new();
    args.insert("pid".to_string(), serde_json::json!(9999));
    match client.call_tool("get_process", Some(args)).await {
        Ok(result) => tracing::info!("🔍 {}", result),
        Err(e) => tracing::info!("❌ Expected error: {}", e),
    }

    // Final process list
    let args = HashMap::new();
    let result = client.call_tool("list_processes", Some(args)).await?;
    tracing::info!("📋 Final: {}", result);

    tracing::info!("🔚 Unix socket client demo completed");
    tracing::info!("💡 Unix sockets provide excellent performance for local IPC!");

    Ok(())
}
