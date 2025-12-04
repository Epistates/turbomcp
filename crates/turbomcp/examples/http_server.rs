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
//!
//! ## Browser-Based Tools (MCP Inspector)
//!
//! By default, CORS is disabled for security. To use browser-based tools like
//! [MCP Inspector](https://github.com/anthropics/mcp-inspector), you need to
//! enable CORS. Set the `ENABLE_CORS` environment variable:
//!
//! ```bash
//! ENABLE_CORS=1 cargo run --example http_server --features http
//! ```
//!
//! **Security Note**: Only enable CORS in development. For production, configure
//! specific allowed origins using `StreamableHttpConfigBuilder::with_allowed_origins()`.

#[cfg(feature = "http")]
use turbomcp::prelude::*;

#[cfg(feature = "http")]
use turbomcp_transport::streamable_http_v2::StreamableHttpConfigBuilder;

#[derive(Clone)]
struct HttpServer;

#[turbomcp::server(name = "http-demo", version = "1.0.0", transports = ["http"])]
impl HttpServer {
    #[tool("Get server info")]
    async fn info(&self) -> McpResult<String> {
        Ok("HTTP/SSE transport server running!".to_string())
    }

    #[tool("Echo a message")]
    async fn echo(&self, message: String) -> McpResult<String> {
        Ok(format!("Echo: {}", message))
    }

    #[tool("Read a mock file content")]
    async fn read_file_tool(&self, file_path: String) -> McpResult<String> {
        if file_path == "/mock/data.txt" {
            Ok("This is the content of the mock data file.".to_string())
        } else {
            Err(McpError::resource_not_found(format!("File not found: {}", file_path)))
        }
    }

    #[resource(
        uri_pattern = "file:///mock/data.txt",
        description = "A mock data file resource"
    )]
    async fn mock_data_resource(&self, _uri: String) -> McpResult<String> {
        Ok("This is the content of the mock data file served as a resource.".to_string())
    }

    #[prompt("Get a personalized greeting")]
    async fn personalized_greeting(&self, name: String) -> McpResult<String> {
        Ok(format!("Hello, {}! Welcome to the HTTP/SSE server.", name))
    }
}

#[tokio::main]
#[cfg(feature = "http")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stdout)
        .init();

    // Check if CORS should be enabled (for browser-based tools like MCP Inspector)
    let enable_cors = std::env::var("ENABLE_CORS").is_ok();

    println!("\n╔══════════════════════════════════════╗");
    println!("║      HTTP/SSE Server Example       ║");
    println!("╚══════════════════════════════════════╝");
    println!("\n🌐 Starting server...");
    println!("📡 Listening on http://localhost:3000/mcp\n");
    println!("Available methods:");
    println!("  • tools/list (POST)");
    println!("  • tools/call (POST) - info, echo, read_file_tool");
    println!("  • resources/read (POST) - file:///mock/data.txt");
    println!("  • prompts/get (POST) - personalized_greeting\n");

    if enable_cors {
        println!("🔓 CORS enabled (development mode)");
        println!("   Browser-based tools like MCP Inspector can connect.\n");
    } else {
        println!("🔒 CORS disabled (default, secure)");
        println!(
            "   To enable for MCP Inspector: ENABLE_CORS=1 cargo run --example http_server --features http\n"
        );
    }

    println!("Test with curl (see docs in example source code)\n");

    tracing::info!("Starting HTTP/SSE server on 127.0.0.1:3000");

    // Build configuration with optional CORS support
    let config = StreamableHttpConfigBuilder::new()
        .with_bind_address("127.0.0.1:3000")
        .with_endpoint_path("/mcp")
        .allow_any_origin(enable_cors) // Enable CORS only when ENABLE_CORS is set
        .build();

    HttpServer
        .run_http_with_config("127.0.0.1:3000", config)
        .await?;

    Ok(())
}

#[cfg(not(feature = "http"))]
fn main() {
    eprintln!(
        "This example requires the 'http' feature. Run with: cargo run --example http_server --features http"
    );
}
