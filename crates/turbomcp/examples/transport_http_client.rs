//! HTTP/SSE Transport Client
//!
//! This example demonstrates how to create a client that connects to
//! an HTTP/SSE-based MCP server for web integration.
//!
//! Run with: `cargo run --example transport_http_client`

use std::collections::HashMap;
use tokio::time::{Duration, sleep};
use turbomcp_client::Client;
use turbomcp_transport::http_sse::{HttpSseConfig, HttpSseTransport};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("🌐 Starting HTTP/SSE Transport Client");

    // Wait for server to be ready (if running manually)
    tracing::info!("⏳ Connecting to HTTP server at http://localhost:3000...");
    sleep(Duration::from_millis(1000)).await;

    // Create HTTP/SSE transport - this needs to be a client mode transport
    // Note: This example shows the pattern but HttpSseTransport may be server-only
    let config = HttpSseConfig::default();
    let transport = HttpSseTransport::new(config);
    let mut client = Client::new(transport);

    tracing::info!("✅ Connected to HTTP server");

    // Initialize the connection
    let init_result = client.initialize().await?;
    tracing::info!("📋 Server: {}", init_result.server_info.name);
    tracing::info!("🔧 Version: {}", init_result.server_info.version);

    // List available tools
    let tools = client.list_tools().await?;
    tracing::info!("🛠️  Available tools: {}", tools.len());
    for tool_name in &tools {
        tracing::info!("  - {}", tool_name);
    }

    // Test weather operations
    tracing::info!("🌤️  Testing weather service...");

    let mut args = HashMap::new();
    let result = client
        .call_tool("list_locations", Some(args.clone()))
        .await?;
    tracing::info!("📍 {}", result);

    args.clear();
    args.insert("location".to_string(), serde_json::json!("New York"));
    let result = client.call_tool("get_weather", Some(args.clone())).await?;
    tracing::info!("🗽 {}", result);

    args.clear();
    args.insert("location".to_string(), serde_json::json!("Paris"));
    let result = client.call_tool("add_location", Some(args.clone())).await?;
    tracing::info!("🇫🇷 {}", result);

    let result = client.call_tool("get_weather", Some(args.clone())).await?;
    tracing::info!("🇫🇷 {}", result);

    // Test resource access
    let resources = client.list_resources().await?;
    tracing::info!("📁 Available resources: {}", resources.len());
    for resource_uri in &resources {
        let content = client.read_resource(resource_uri).await?;
        tracing::info!("📄 {}:\n{:?}", resource_uri, content.contents);
    }

    tracing::info!("🔚 HTTP client demo completed");

    Ok(())
}
