//! TCP Transport Client - Minimal Example
//!
//! Connects to TCP server and calls tools.
//!
//! **Run server first:**
//! ```bash
//! cargo run --example tcp_server --features tcp
//! ```
//!
//! **Then run client:**
//! ```bash
//! cargo run --example tcp_client --features tcp
//! ```

use std::collections::HashMap;
use turbomcp_client::Client;
use turbomcp_transport::tcp::TcpTransport;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("🔌 Connecting to TCP server...");

    // Create TCP client transport
    let bind_addr = "127.0.0.1:0".parse()?;
    let server_addr = "127.0.0.1:8765".parse()?;
    let transport = TcpTransport::new_client(bind_addr, server_addr);
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
    args.insert("message".to_string(), serde_json::json!("Hello TCP!"));
    let result = client.call_tool("echo", Some(args)).await?;
    tracing::info!("📝 Echo result: {:?}", result);

    // Call add tool
    let mut args = HashMap::new();
    args.insert("a".to_string(), serde_json::json!(10.5));
    args.insert("b".to_string(), serde_json::json!(20.3));
    let result = client.call_tool("add", Some(args)).await?;
    tracing::info!("🔢 Add result: {:?}", result);

    tracing::info!("✅ Demo complete");
    Ok(())
}
