//! WebSocket Transport Client - Minimal Example
//!
//! Connects to WebSocket server and calls tools.
//!
//! **Run server first:**
//! ```bash
//! cargo run --example websocket_server --features "http,websocket"
//! ```
//!
//! **Then run client:**
//! ```bash
//! cargo run --example websocket_client --features "http,websocket"
//! ```

use std::collections::HashMap;
use turbomcp_client::Client;
use turbomcp_transport::websocket_bidirectional::{
    WebSocketBidirectionalConfig, WebSocketBidirectionalTransport,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("🔌 Connecting to WebSocket server...");

    // Create WebSocket client transport
    let config = WebSocketBidirectionalConfig::new("ws://127.0.0.1:8080");
    let transport = WebSocketBidirectionalTransport::new(config).await?;
    let client = Client::new(transport);

    // Initialize (auto-connects)
    let init = client.initialize().await?;
    tracing::info!("✅ Connected to: {}", init.server_info.name);

    // List tools
    let tools = client.list_tools().await?;
    tracing::info!("🛠️  Found {} tools:", tools.len());
    for tool in &tools {
        tracing::info!("  - {}: {}", tool.name, tool.description.as_deref().unwrap_or(""));
    }

    // Call echo tool
    let mut args = HashMap::new();
    args.insert("message".to_string(), serde_json::json!("Hello WebSocket!"));
    let result = client.call_tool("echo", Some(args)).await?;
    tracing::info!("📝 Echo result: {:?}", result);

    // Call timestamp tool
    let result = client.call_tool("timestamp", None).await?;
    tracing::info!("🕐 Timestamp result: {:?}", result);

    // List resources
    let resources = client.list_resources().await?;
    tracing::info!("📦 Found {} resources:", resources.len());
    for resource in &resources {
        tracing::info!("  - {}", resource.uri);
    }

    tracing::info!("✅ Demo complete");
    Ok(())
}
