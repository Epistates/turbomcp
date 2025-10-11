//! Unix Socket Transport Client - Minimal Example
//!
//! Connects to Unix socket server and calls tools.
//!
//! **Run server first:**
//! ```bash
//! cargo run --example unix_server --features unix
//! ```
//!
//! **Then run client:**
//! ```bash
//! cargo run --example unix_client --features unix
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use turbomcp_client::{Client, Result};
use turbomcp_transport::UnixTransport;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("🔌 Connecting to Unix socket server...");

    let socket_path = PathBuf::from("/tmp/turbomcp-demo.sock");

    // Create Unix socket client transport
    let transport = UnixTransport::new_client(socket_path);
    let client = Client::new(transport);

    // Initialize (auto-connects)
    let init = client.initialize().await?;
    tracing::info!("✅ Connected to: {}", init.server_info.name);

    // List tools
    let tools = client.list_tools().await?;
    tracing::info!("🛠️  Found {} tools:", tools.len());
    for tool in &tools {
        tracing::info!(
            "  - {}: {}",
            tool.name,
            tool.description.as_deref().unwrap_or("")
        );
    }

    // Call echo tool
    let mut args = HashMap::new();
    args.insert(
        "message".to_string(),
        serde_json::json!("Hello Unix Socket!"),
    );
    let result = client.call_tool("echo", Some(args)).await?;
    tracing::info!("📝 Echo result: {:?}", result);

    // Call multiply tool
    let mut args = HashMap::new();
    args.insert("a".to_string(), serde_json::json!(7.0));
    args.insert("b".to_string(), serde_json::json!(6.0));
    let result = client.call_tool("multiply", Some(args)).await?;
    tracing::info!("🔢 Multiply result: {:?}", result);

    // List and read resources
    let resources = client.list_resources().await?;
    tracing::info!("📦 Found {} resources:", resources.len());
    for resource_uri in &resources {
        tracing::info!("  - {}", resource_uri);
        let content = client.read_resource(resource_uri).await?;
        tracing::info!("    Content: {:?}", content);
    }

    tracing::info!("✅ Demo complete");
    Ok(())
}
