//! Demonstration of explicit transport selection with the transports attribute.
//!
//! This example shows:
//! 1. Backward compatibility: No transports specified → generates all enabled transports
//! 2. Explicit transports: Specify which transports to include in the server
//! 3. Multiple discrete servers with different transports in one binary
//!
//! Run with:
//! ```bash
//! cargo run --example transports_demo --features "stdio,http,tcp"
//! ```

use turbomcp::prelude::*;

/// Backward-compatible server - generates all enabled transports by default
/// When no transports are specified, the macro behaves exactly as before,
/// creating methods for all enabled features (http, websocket, tcp, unix)
#[derive(Clone)]
struct AllTransportsServer;

#[turbomcp::server(
    name = "all-transports",
    version = "1.0",
    description = "Default server supporting all enabled transports"
)]
impl AllTransportsServer {
    #[tool(description = "Greet someone")]
    async fn greet_all(&self, name: String) -> McpResult<String> {
        Ok(format!("Hello {} from all-transports!", name))
    }
}

/// HTTP-only server - explicitly restricted to HTTP transport
/// This server will only have the run_http() method available,
/// even if other transports are enabled in Cargo.toml
#[derive(Clone)]
struct HttpOnlyServer;

#[turbomcp::server(
    name = "http-only",
    version = "1.0",
    description = "Server that only supports HTTP transport",
    transports = ["http"]
)]
impl HttpOnlyServer {
    #[tool(description = "Make an API call")]
    async fn api_call(&self, query: String) -> McpResult<String> {
        Ok(format!("API result for: {}", query))
    }
}

/// TCP-only server - explicitly restricted to TCP transport
/// This server will only have the run_tcp() method available
#[derive(Clone)]
struct TcpOnlyServer;

#[turbomcp::server(
    name = "tcp-only",
    version = "1.0",
    description = "Server that only supports TCP transport",
    transports = ["tcp"]
)]
impl TcpOnlyServer {
    #[tool(description = "Execute a network command")]
    async fn network_cmd(&self, cmd: String) -> McpResult<String> {
        Ok(format!("TCP command executed: {}", cmd))
    }
}

/// Multi-transport server - explicitly includes HTTP and TCP
/// This server will have run_http() and run_tcp() methods,
/// but not run_websocket() or run_unix() even if enabled
#[derive(Clone)]
struct HttpTcpServer;

#[turbomcp::server(
    name = "http-tcp",
    version = "1.0",
    description = "Server supporting HTTP and TCP transports",
    transports = ["http", "tcp"]
)]
impl HttpTcpServer {
    #[tool(description = "Process data via HTTP or TCP")]
    async fn multi_transport(&self, data: String) -> McpResult<String> {
        Ok(format!("Processed via HTTP/TCP: {}", data))
    }
}

/// Demonstrates the use case of multiple discrete servers in one binary
/// Each server has a specific purpose and only the transports it needs.
/// This reduces:
/// - Compilation time (fewer methods generated)
/// - API surface (users only see methods they can use)
/// - Confusion (clear intent about which transports are supported)
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== TurboMCP Transports Demonstration ===\n");

    // Example 1: Backward compatibility (no transports specified)
    println!("1. Backward-compatible server (all transports):");
    println!("   - AllTransportsServer has: run_stdio(), run_http(), run_tcp(), etc.");
    println!("   - No transports attribute = uses all enabled features\n");

    // Example 2: Explicit single transport
    println!("2. HTTP-only server (transports = [\"http\"]):");
    println!("   - HttpOnlyServer has: run_http() ONLY");
    println!("   - No run_tcp(), run_websocket(), run_unix() methods\n");

    // Example 3: Explicit single transport
    println!("3. TCP-only server (transports = [\"tcp\"]):");
    println!("   - TcpOnlyServer has: run_tcp() ONLY");
    println!("   - No run_http(), run_websocket(), run_unix() methods\n");

    // Example 4: Multiple explicit transports
    println!("4. HTTP+TCP server (transports = [\"http\", \"tcp\"]):");
    println!("   - HttpTcpServer has: run_http(), run_tcp()");
    println!("   - No run_websocket(), run_unix() methods\n");

    println!("=== Use Cases ===\n");

    println!("Case 1: Public API server");
    println!("  #[server(name = \"api\", transports = [\"http\"])]");
    println!("  - Only expose HTTP interface to clients");
    println!("  - Smaller API surface\n");

    println!("Case 2: Internal service (same machine)");
    println!("  #[server(name = \"internal\", transports = [\"unix\"])]");
    println!("  - Use Unix sockets for local IPC");
    println!("  - Lower overhead than TCP\n");

    println!("Case 3: Hybrid deployment");
    println!("  #[server(name = \"hybrid\", transports = [\"http\", \"tcp\"])]");
    println!("  - Public HTTP API for web clients");
    println!("  - TCP for internal services\n");

    println!("Case 4: Backward compatibility (existing code)");
    println!("  #[server(name = \"legacy\")] // no transports specified");
    println!("  - Works exactly like previous versions");
    println!("  - Generates all enabled transports automatically\n");

    println!("=== Validation ===\n");
    println!("Valid transports: http, websocket, tcp, unix");
    println!("Invalid transports will cause compile-time error:\n");
    println!("  #[server(transports = [\"invalid\"])]");
    println!("  → error: Invalid transport 'invalid'. Valid transports are: http, websocket, tcp, unix\n");

    Ok(())
}
