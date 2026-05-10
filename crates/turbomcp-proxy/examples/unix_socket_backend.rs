//! Example: Unix Domain Socket Backend Proxy
//!
//! Demonstrates connecting to an MCP server via Unix domain socket and printing its capabilities.
//! Unix sockets provide efficient IPC (Inter-Process Communication) with security isolation.
//!
//! Usage:
//!   1. Start an MCP server listening on Unix socket at /tmp/turbomcp-demo.sock
//!   2. Run: cargo run --example unix_socket_backend
//!   3. Inspect the backend capabilities printed by this example
//!
//! Example MCP server startup:
//!   ```bash
//!   cargo run -p turbomcp --example unix_server --features unix
//!   ```

#[cfg(not(unix))]
fn main() {
    eprintln!("This example requires a Unix platform.");
}

#[cfg(unix)]
use std::path::PathBuf;
#[cfg(unix)]
use turbomcp_proxy::proxy::{BackendConfig, BackendConnector, BackendTransport};

#[cfg(unix)]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("🚀 Unix Domain Socket Backend Proxy Example");
    println!("===========================================\n");

    let socket_path = "/tmp/turbomcp-demo.sock";

    // Check if socket exists
    if !PathBuf::from(socket_path).exists() {
        eprintln!("❌ Socket file not found at {}", socket_path);
        eprintln!("\nTo run this example:");
        eprintln!("  1. Start an MCP server listening on Unix socket:");
        eprintln!("     cargo run -p turbomcp --example unix_server --features unix");
        eprintln!("  2. Then run this example");
        return Err("Socket not found".into());
    }

    // Configure Unix socket backend connection
    let backend_config = BackendConfig {
        transport: BackendTransport::Unix {
            path: socket_path.to_string(),
        },
        client_name: "unix-socket-proxy-example".to_string(),
        client_version: "1.0.0".to_string(),
    };

    println!("📡 Connecting to Unix socket at {}...", socket_path);

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

    println!("\n✨ Unix socket backend proxy is ready!");
    println!("\n💡 Benefits of Unix sockets:");
    println!("  - Efficient IPC (Inter-Process Communication)");
    println!("  - Security isolation with filesystem permissions");
    println!("  - No network overhead for same-host connections");
    println!("  - Perfect for containerized applications");

    println!("\nIn a production scenario, you would now:");
    println!("  1. Wrap this backend in a ProxyService");
    println!("  2. Expose it over HTTP with Axum");
    println!(
        "  3. Run: turbomcp-proxy serve --backend unix --unix {} --frontend http --bind 127.0.0.1:3002",
        socket_path
    );

    Ok(())
}
