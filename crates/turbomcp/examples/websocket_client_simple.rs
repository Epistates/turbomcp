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

#[cfg(all(feature = "http", feature = "websocket"))]
use std::collections::HashMap;
#[cfg(all(feature = "http", feature = "websocket"))]
use turbomcp_client::Client;
#[cfg(all(feature = "http", feature = "websocket"))]
use turbomcp_transport::websocket_bidirectional::{
    WebSocketBidirectionalConfig, WebSocketBidirectionalTransport,
};

#[tokio::main]
#[cfg(all(feature = "http", feature = "websocket"))]
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
        url: Some("ws://127.0.0.1:8080/ws".to_string()),
        ..Default::default()
    };
    let transport = WebSocketBidirectionalTransport::new(config).await?;

    eprintln!("[1/4] 🔌 Connecting to ws://127.0.0.1:8080/ws...");
    let client = Client::new(transport);

    // Initialize (auto-connects)
    let init = client.initialize().await?;
    eprintln!(
        "[2/4] ✅ Connected: {} v{}",
        init.server_info.name, init.server_info.version
    );

    // List and call tools
    eprintln!("\n[3/4] 🛠️  Listing tools...");
    let tools = client.list_tools().await?;
    for tool in &tools {
        eprintln!(
            "  • {}: {}",
            tool.name,
            tool.description.as_deref().unwrap_or("")
        );
    }

    eprintln!("\n[4/4] 📞 Calling tools...");

    // Call echo tool if available
    if tools.iter().any(|t| t.name == "echo") {
        let mut args = HashMap::new();
        args.insert("message".to_string(), serde_json::json!("Hello WebSocket!"));
        let result = client.call_tool("echo", Some(args)).await?;
        eprintln!("  → echo: {}", result);
    }

    // Call add tool if available
    if tools.iter().any(|t| t.name == "add") {
        let mut args = HashMap::new();
        args.insert("a".to_string(), serde_json::json!(15));
        args.insert("b".to_string(), serde_json::json!(27));
        let result = client.call_tool("add", Some(args)).await?;
        eprintln!("  → add: {}", result);
    }

    eprintln!("\n✅ Demo complete!\n");
    Ok(())
}

#[cfg(not(all(feature = "http", feature = "websocket")))]
fn main() {
    eprintln!(
        "This example requires 'http' and 'websocket' features. Run with: cargo run --example websocket_client_simple --features \"http,websocket\""
    );
}
