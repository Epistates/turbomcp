//! TCP Transport Client - Minimal Example
//!
//! Connects to TCP server and demonstrates basic MCP operations.
//!
//! **Run server first:**
//! ```bash
//! cargo run --example tcp_server --features tcp
//! ```
//!
//! **Then run client:**
//! ```bash
//! cargo run --example tcp_client_simple --features tcp
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

    eprintln!("\n╔══════════════════════════════════════╗");
    eprintln!("║   TCP Transport Client Demo         ║");
    eprintln!("╚══════════════════════════════════════╝\n");

    // Create TCP transport
    let bind_addr = "127.0.0.1:0".parse()?;  // 0 = OS assigns port
    let server_addr = "127.0.0.1:8765".parse()?;
    let transport = TcpTransport::new_client(bind_addr, server_addr);

    eprintln!("[1/4] 🔌 Connecting to tcp://127.0.0.1:8765...");
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
    args.insert("message".to_string(), serde_json::json!("Hello TCP!"));
    let result = client.call_tool("echo", Some(args)).await?;
    eprintln!("  → echo: {}", result);

    // Call add tool
    let mut args = HashMap::new();
    args.insert("a".to_string(), serde_json::json!(10));
    args.insert("b".to_string(), serde_json::json!(32));
    let result = client.call_tool("add", Some(args)).await?;
    eprintln!("  → add: {}", result);

    eprintln!("\n✅ Demo complete!\n");
    Ok(())
}
