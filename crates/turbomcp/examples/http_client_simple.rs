//! # HTTP/SSE Transport Client - Minimal Example
//!
//! Demonstrates connecting to an HTTP/SSE server and performing basic MCP operations:
//! - Initialize connection with server
//! - List available tools
//! - Call tools with arguments
//!
//! ## Quick Start
//!
//! **Terminal 1: Start the server**
//! ```bash
//! cargo run --example http_server --features http
//! ```
//!
//! **Terminal 2: Run the client**
//! ```bash
//! cargo run --example http_client_simple --features http
//! ```

use std::collections::HashMap;
use std::time::Duration;

#[cfg(feature = "http")]
use turbomcp_client::{Client, Result};
#[cfg(feature = "http")]
use turbomcp_transport::streamable_http_client::{
    StreamableHttpClientConfig, StreamableHttpClientTransport,
};

#[tokio::main]
#[cfg(feature = "http")]
async fn main() -> Result<()> {

    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stdout)
        .init();

    println!("\n╔══════════════════════════════════════╗");
    println!("║   HTTP/SSE Transport Client Demo    ║");
    println!("╚══════════════════════════════════════╝\n");

    // Create HTTP transport
    let config = StreamableHttpClientConfig {
        base_url: "http://localhost:3000".to_string(),
        endpoint_path: "/mcp".to_string(),
        timeout: Duration::from_secs(30),
        ..Default::default()
    };
    let transport = StreamableHttpClientTransport::new(config);

    println!("[1/4] 🔌 Connecting to http://localhost:3000/mcp...");
    let client = Client::new(transport);

    // Initialize
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

    // Call echo tool
    let mut args = HashMap::new();
    args.insert("message".to_string(), serde_json::json!("Hello HTTP!"));
    let result = client.call_tool("echo", Some(args)).await?;
    println!("  → echo: {}", result);

    // Call info tool
    let result = client.call_tool("info", None).await?;
    println!("  → info: {}", result);

    println!("\n✅ Demo complete!\n");
    Ok(())
}

#[cfg(not(feature = "http"))]
fn main() {
    eprintln!(
        "This example requires the 'http' feature. Run with: cargo run --example http_client_simple --features http"
    );
}
