//! WebSocket Transport Client - Minimal Example
//!
//! Connects to WebSocket server and demonstrates basic MCP operations.
//!
//! **Run server first:**
//! ```bash
//! cargo run --example websocket_server_simple --features "http,websocket"
//! ```
//!
//! **Then run client:**
//! ```bash
//! cargo run --example websocket_client_simple --features "http,websocket"
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

    eprintln!("\n╔══════════════════════════════════════╗");
    eprintln!("║   WebSocket Transport Client Demo   ║");
    eprintln!("╚══════════════════════════════════════╝\n");

    // Create WebSocket transport
    let config = WebSocketBidirectionalConfig {
        url: Some("ws://127.0.0.1:8080".to_string()),
        ..Default::default()
    };
    let transport = WebSocketBidirectionalTransport::new(config).await?;

    eprintln!("[1/4] 🔌 Connecting to ws://127.0.0.1:8080...");
    let client = Client::new(transport);

    // Initialize (auto-connects)
    let init = client.initialize().await?;
    eprintln!("[2/4] ✅ Connected: {} v{}", init.server_info.name, init.server_info.version);

    // List and call tools
    eprintln!("\n[3/4] 🛠️  Listing tools...");
    let tools = client.list_tools().await?;
    for tool in &tools {
        eprintln!("  • {}: {}", tool.name, tool.description.as_deref().unwrap_or(""));
    }

    eprintln!("\n[4/4] 📞 Calling tools...");

    // Call echo tool
    let mut args = HashMap::new();
    args.insert("message".to_string(), serde_json::json!("Hello WebSocket!"));
    let result = client.call_tool("echo", Some(args)).await?;
    eprintln!("  → echo: {}", result);

    // Call timestamp tool
    let result = client.call_tool("timestamp", None).await?;
    eprintln!("  → timestamp: {}", result);

    eprintln!("\n✅ Demo complete!\n");
    Ok(())
}
