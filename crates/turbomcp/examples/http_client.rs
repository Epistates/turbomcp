//! HTTP Transport Client - Minimal Example
//!
//! Connects to HTTP/SSE server and calls tools.
//!
//! **Run server first:**
//! ```bash
//! cargo run --example http_server --features http
//! ```
//!
//! **Then run client:**
//! ```bash
//! cargo run --example http_client --features http
//! ```

use std::collections::HashMap;
use turbomcp_client::Client;
use turbomcp_transport::streamable_http_client::{
    HttpTransportConfig, StreamableHttpClientTransport,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("🔌 Connecting to HTTP server...");

    // Create HTTP client transport
    let config = HttpTransportConfig::new("http://127.0.0.1:3000");
    let transport = StreamableHttpClientTransport::new(config)?;
    let client = Client::new(transport);

    // Initialize (auto-connects)
    let init = client.initialize().await?;
    tracing::info!("✅ Connected to: {}", init.server_info.name);
    tracing::info!("   Version: {}", init.protocol_version);

    // List tools
    let tools = client.list_tools().await?;
    tracing::info!("🛠️  Found {} tools:", tools.len());
    for tool in &tools {
        tracing::info!("  - {}: {}", tool.name, tool.description.as_deref().unwrap_or(""));
    }

    // Call echo tool
    let mut args = HashMap::new();
    args.insert("message".to_string(), serde_json::json!("Hello HTTP!"));
    let result = client.call_tool("echo", Some(args)).await?;
    tracing::info!("📝 Echo result: {:?}", result);

    // Call info tool
    let result = client.call_tool("info", None).await?;
    tracing::info!("ℹ️  Info result: {:?}", result);

    // List resources
    let resources = client.list_resources().await?;
    tracing::info!("📦 Found {} resources:", resources.len());
    for resource in &resources {
        tracing::info!("  - {}", resource.uri);
    }

    // Read a resource
    if let Some(resource) = resources.first() {
        let content = client.read_resource(&resource.uri).await?;
        tracing::info!("📄 Resource content: {:?}", content);
    }

    tracing::info!("✅ Demo complete");
    Ok(())
}
