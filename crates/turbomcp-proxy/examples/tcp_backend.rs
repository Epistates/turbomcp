//! Example: TCP Backend Proxy
//!
//! Demonstrates connecting to an MCP server via TCP and exposing it over HTTP.
//!
//! Usage:
//!   1. Start an MCP server on TCP port 5000
//!   2. Run: cargo run --example tcp_backend
//!   3. Connect to HTTP at http://localhost:3001/mcp
//!
//! Example MCP server startup (if you have one):
//!   ```bash
//!   your-mcp-server --listen-tcp localhost:5000
//!   ```

use turbomcp_proxy::proxy::{BackendConfig, BackendConnector, BackendTransport};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("🚀 TCP Backend Proxy Example");
    println!("============================\n");

    // Configure TCP backend connection
    let backend_config = BackendConfig {
        transport: BackendTransport::Tcp {
            host: "localhost".to_string(),
            port: 5000,
        },
        client_name: "tcp-proxy-example".to_string(),
        client_version: "1.0.0".to_string(),
    };

    println!("📡 Connecting to TCP server at localhost:5000...");

    // Create backend connector (establishes connection and initializes)
    let backend = BackendConnector::new(backend_config).await?;
    println!("✅ Connected to backend successfully");

    // Introspect server capabilities
    println!("\n🔍 Introspecting server capabilities...");
    let spec = backend.introspect().await?;

    println!("✅ Introspection complete");
    println!("   Server: {}", spec.server_info.name);
    println!("   Version: {}", spec.server_info.version);
    println!("   Tools: {}", spec.tools.len());
    println!("   Resources: {}", spec.resources.len());
    println!("   Prompts: {}", spec.prompts.len());

    // List available tools
    if !spec.tools.is_empty() {
        println!("\n📋 Available Tools:");
        for tool in &spec.tools {
            println!("   - {}", tool.name);
            if let Some(desc) = &tool.description {
                println!("     {}", desc);
            }
        }
    }

    // List available resources
    if !spec.resources.is_empty() {
        println!("\n📂 Available Resources:");
        for resource in &spec.resources {
            println!("   - {}", resource.uri);
            if let Some(desc) = &resource.description {
                println!("     {}", desc);
            }
        }
    }

    println!("\n✨ TCP backend proxy is ready!");
    println!("In a production scenario, you would now:");
    println!("  1. Wrap this backend in a ProxyService");
    println!("  2. Expose it over HTTP with Axum");
    println!(
        "  3. Run: turbomcp-proxy serve --backend tcp --tcp localhost:5000 --frontend http --bind 127.0.0.1:3001"
    );

    Ok(())
}
