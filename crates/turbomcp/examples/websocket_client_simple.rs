//! # WebSocket Transport Client - Minimal Example
//!
//! Demonstrates connecting to a WebSocket server and performing basic MCP operations:
//! - Initialize connection with server
//! - List available tools
//! - Call tools with arguments
//!
//! ## Quick Start
//!
//! **Terminal 1: Start the server**
//! ```bash
//! cargo run --example websocket_server --features "http,websocket"
//! ```
//!
//! **Terminal 2: Run the client**
//! ```bash
//! cargo run --example websocket_client_simple --features "http,websocket"
//! ```

#[cfg(all(feature = "http", feature = "websocket"))]
use turbomcp_client::Client;
#[cfg(all(feature = "http", feature = "websocket"))]
use turbomcp_transport::websocket_bidirectional::{
    WebSocketBidirectionalConfig, WebSocketBidirectionalTransport,
};

#[tokio::main]
#[cfg(all(feature = "http", feature = "websocket"))]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stdout)
        .init();

    println!("\n╔══════════════════════════════════════╗");
    println!("║   WebSocket Transport Client Demo   ║");
    println!("╚══════════════════════════════════════╝\n");

    // Create WebSocket transport
    let config = WebSocketBidirectionalConfig {
        url: Some("ws://127.0.0.1:8080/ws".to_string()),
        ..Default::default()
    };
    let transport = WebSocketBidirectionalTransport::new(config).await?;

    println!("[1/4] 🔌 Connecting to ws://127.0.0.1:8080/ws...");
    let client = Client::new(transport);

    // Initialize (auto-connects)
    let init = client.initialize().await?;
    println!(
        "[2/4] ✅ Connected: {} v{}",
        init.server_info.name, init.server_info.version
    );

    // List and call tools
    println!("\n[3/4] 🛠️  Listing tools...");
    let tools = client.list_tools().await?;
    for tool in &tools {
        println!(
            "  • {}: {}",
            tool.name,
            tool.description.as_deref().unwrap_or("")
        );
    }

    println!("\n[4/4] 📞 Calling tools...");

    // Call echo tool if available
    if tools.iter().any(|t| t.name == "echo") {
        let mut args = HashMap::new();
        args.insert("message".to_string(), serde_json::json!("Hello WebSocket!"));
        let result = client.call_tool("echo", Some(args)).await?;
        println!("  → echo: {}", result);
    }

    // Call add tool if available
    if tools.iter().any(|t| t.name == "add") {
        let mut args = HashMap::new();
        args.insert("a".to_string(), serde_json::json!(15));
        args.insert("b".to_string(), serde_json::json!(27));
        let result = client.call_tool("add", Some(args)).await?;
        println!("  → add: {}", result);
    }

    println!("\n✅ Demo complete!\n");
    Ok(())
}

#[cfg(not(all(feature = "http", feature = "websocket")))]
fn main() {
    eprintln!(
        "This example requires 'http' and 'websocket' features. Run with: cargo run --example websocket_client_simple --features \"http,websocket\""
    );
}
