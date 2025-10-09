//! Unix Socket Transport Client - Minimal Example
//!
//! Connects to Unix socket server and demonstrates basic MCP operations.
//!
//! **Run server first:**
//! ```bash
//! cargo run --example unix_server_simple --features unix
//! ```
//!
//! **Then run client:**
//! ```bash
//! cargo run --example unix_client_simple --features unix
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use turbomcp_client::Client;
use turbomcp_transport::unix::UnixTransport;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    eprintln!("\n╔══════════════════════════════════════╗");
    eprintln!("║   Unix Socket Transport Client Demo ║");
    eprintln!("╚══════════════════════════════════════╝\n");

    let socket_path = PathBuf::from("/tmp/turbomcp-demo.sock");

    // Create Unix socket transport
    let transport = UnixTransport::new_client(socket_path.clone());

    eprintln!("[1/4] 🔌 Connecting to {}...", socket_path.display());
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
    args.insert("message".to_string(), serde_json::json!("Hello Unix Socket!"));
    let result = client.call_tool("echo", Some(args)).await?;
    eprintln!("  → echo: {}", result);

    // Call multiply tool
    let mut args = HashMap::new();
    args.insert("a".to_string(), serde_json::json!(7.0));
    args.insert("b".to_string(), serde_json::json!(6.0));
    let result = client.call_tool("multiply", Some(args)).await?;
    eprintln!("  → multiply: {}", result);

    eprintln!("\n✅ Demo complete!\n");
    Ok(())
}
