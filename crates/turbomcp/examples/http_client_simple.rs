//! HTTP/SSE Transport Client - Minimal Example
//!
//! Connects to HTTP/SSE server and demonstrates basic MCP operations.
//!
//! **Run server first:**
//! ```bash
//! cargo run --example http_server --features http
//! ```
//!
//! **Then run client:**
//! ```bash
//! cargo run --example http_client_simple --features http
//! ```


#[cfg(feature = "http")]
use std::collections::HashMap;
#[cfg(feature = "http")]
use std::time::Duration;
#[cfg(feature = "http")]
use turbomcp_client::Client;
#[cfg(feature = "http")]
use turbomcp_transport::streamable_http_client::{
    StreamableHttpClientConfig, StreamableHttpClientTransport,
};

#[tokio::main]
#[cfg(feature = "http")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    eprintln!("\n╔══════════════════════════════════════╗");
    eprintln!("║   HTTP/SSE Transport Client Demo    ║");
    eprintln!("╚══════════════════════════════════════╝\n");

    // Create HTTP transport
    let config = StreamableHttpClientConfig {
        base_url: "http://localhost:3000".to_string(),
        endpoint_path: "/mcp".to_string(),
        timeout: Duration::from_secs(30),
        ..Default::default()
    };
    let transport = StreamableHttpClientTransport::new(config);

    eprintln!("[1/4] 🔌 Connecting to http://localhost:3000/mcp...");
    let client = Client::new(transport);

    // Initialize
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

    // Call echo tool
    let mut args = HashMap::new();
    args.insert("message".to_string(), serde_json::json!("Hello HTTP!"));
    let result = client.call_tool("echo", Some(args)).await?;
    eprintln!("  → echo: {}", result);

    // Call info tool
    let result = client.call_tool("info", None).await?;
    eprintln!("  → info: {}", result);

    eprintln!("\n✅ Demo complete!\n");
    Ok(())
}

#[cfg(not(feature = "http"))]
fn main() {
    eprintln!("This example requires the 'http' feature. Run with: cargo run --example http_client_simple --features http");
}
