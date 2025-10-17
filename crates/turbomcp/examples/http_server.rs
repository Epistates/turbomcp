//! # HTTP/SSE Server - Minimal Example
//!
//! Demonstrates HTTP transport with Server-Sent Events (SSE) for web compatibility.
//! This is the simplest way to expose an MCP server over HTTP for web clients.
//!
//! ## Quick Start
//!
//! ```bash
//! cargo run --example http_server --features http
//! ```
//!
//! ## Testing
//!
//! In another terminal, test with curl:
//! ```bash
//! # List tools
//! curl -X POST http://localhost:3000/mcp \
//!   -H "Content-Type: application/json" \
//!   -d '{"jsonrpc":"2.0","method":"tools/list","id":1}'
//!
//! # Call the echo tool
//! curl -X POST http://localhost:3000/mcp \
//!   -H "Content-Type: application/json" \
//!   -d '{"jsonrpc":"2.0","method":"tools/call","params":{"name":"echo","arguments":{"message":"Hello"}},"id":2}'
//! ```

#[cfg(feature = "http")]
use turbomcp::prelude::*;

#[derive(Clone)]
struct HttpServer;

#[turbomcp::server(name = "http-demo", version = "1.0.0")]
impl HttpServer {
    #[tool("Get server info")]
    async fn info(&self) -> McpResult<String> {
        Ok("HTTP/SSE transport server running!".to_string())
    }

    #[tool("Echo a message")]
    async fn echo(&self, message: String) -> McpResult<String> {
        Ok(format!("Echo: {}", message))
    }
}

#[tokio::main]
#[cfg(feature = "http")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stdout)
        .init();

    println!("\n╔══════════════════════════════════════╗");
    println!("║      HTTP/SSE Server Example       ║");
    println!("╚══════════════════════════════════════╝");
    println!("\n🌐 Starting server...");
    println!("📡 Listening on http://localhost:3000/mcp\n");
    println!("Available tools:");
    println!("  • info - Get server info");
    println!("  • echo - Echo a message back\n");
    println!("Test with curl (see docs in example source code)\n");

    tracing::info!("Starting HTTP/SSE server on 127.0.0.1:3000");

    HttpServer
        .run_http_with_path("127.0.0.1:3000", "/mcp")
        .await?;

    Ok(())
}

#[cfg(not(feature = "http"))]
fn main() {
    eprintln!(
        "This example requires the 'http' feature. Run with: cargo run --example http_server --features http"
    );
}
