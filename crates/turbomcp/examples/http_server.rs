//! # HTTP/SSE Server - Minimal Example
//!
//! Demonstrates HTTP transport with Server-Sent Events (SSE) for web compatibility.
//! This is the simplest way to expose an MCP server over HTTP for web clients.
//!
//! ## Quick Start (HTTP)
//!
//! ```bash
//! cargo run --example http_server --features http
//! ```
//!
//! ## Quick Start (HTTPS)
//!
//! First, generate a self-signed certificate for development:
//! ```bash
//! openssl req -x509 -newkey rsa:4096 -keyout key.pem -out cert.pem \
//!     -days 365 -nodes -subj "/CN=localhost"
//! ```
//!
//! Then run with TLS enabled:
//! ```bash
//! ENABLE_TLS=1 cargo run --example http_server --features "http,tls"
//! ```
//!
//! ## Testing
//!
//! In another terminal, test with curl:
//! ```bash
//! # HTTP mode
//! curl -X POST http://localhost:3000/mcp \
//!   -H "Content-Type: application/json" \
//!   -d '{"jsonrpc":"2.0","method":"tools/list","id":1}'
//!
//! # HTTPS mode (use -k to skip certificate verification for self-signed certs)
//! curl -k -X POST https://localhost:3000/mcp \
//!   -H "Content-Type: application/json" \
//!   -d '{"jsonrpc":"2.0","method":"tools/list","id":1}'
//! ```
//!
//! ## Browser-Based Tools (MCP Inspector)
//!
//! By default, CORS is disabled for security. To use browser-based tools like
//! [MCP Inspector](https://github.com/anthropics/mcp-inspector), you need to
//! enable CORS. Set the `ENABLE_CORS` environment variable:
//!
//! ```bash
//! # HTTP with CORS
//! ENABLE_CORS=1 cargo run --example http_server --features http
//!
//! # HTTPS with CORS
//! ENABLE_CORS=1 ENABLE_TLS=1 cargo run --example http_server --features "http,tls"
//! ```
//!
//! Then connect MCP Inspector to:
//! - HTTP: `http://localhost:3000/sse`
//! - HTTPS: `https://localhost:3000/sse`
//!
//! **Security Note**: Only enable CORS in development. For production, configure
//! specific allowed origins using `StreamableHttpConfigBuilder::with_allowed_origins()`.

#[cfg(feature = "http")]
use turbomcp::prelude::*;

#[cfg(feature = "http")]
use turbomcp_transport::streamable_http::StreamableHttpConfigBuilder;

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
            Err(McpError::resource(format!("File not found: {}", file_path)))
        }
    }

    #[resource(
        uri = "file:///mock/data.txt",
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

    // Check if TLS/HTTPS should be enabled
    #[cfg(feature = "tls")]
    let enable_tls = std::env::var("ENABLE_TLS").is_ok();
    #[cfg(not(feature = "tls"))]
    let enable_tls = false;

    let scheme = if enable_tls { "https" } else { "http" };

    println!("\n╔══════════════════════════════════════╗");
    println!("║      HTTP/SSE Server Example       ║");
    println!("╚══════════════════════════════════════╝");
    println!("\n🌐 Starting server...");
    println!("📡 Listening on {}://localhost:3000/mcp\n", scheme);
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

    #[cfg(feature = "tls")]
    if enable_tls {
        println!("🔐 TLS/HTTPS enabled");
        println!("   Using cert.pem and key.pem from current directory");
        println!(
            "   Generate with: openssl req -x509 -newkey rsa:4096 -keyout key.pem -out cert.pem -days 365 -nodes -subj \"/CN=localhost\"\n"
        );
    } else {
        println!("📝 TLS disabled (HTTP mode)");
        println!(
            "   To enable HTTPS: ENABLE_TLS=1 cargo run --example http_server --features \"http,tls\"\n"
        );
    }

    #[cfg(not(feature = "tls"))]
    {
        println!("📝 TLS feature not compiled");
        println!("   To enable HTTPS: cargo run --example http_server --features \"http,tls\"\n");
    }

    println!("Test with curl (see docs in example source code)\n");

    tracing::info!("Starting HTTP/SSE server on 127.0.0.1:3000");

    // Build configuration with optional CORS and TLS support
    let mut builder = StreamableHttpConfigBuilder::new()
        .with_bind_address("127.0.0.1:3000")
        .with_endpoint_path("/mcp")
        .allow_any_origin(enable_cors);

    // Add TLS configuration if enabled
    // Certificate and key paths can be customized via environment variables:
    //   TLS_CERT_FILE - path to certificate file (default: cert.pem)
    //   TLS_KEY_FILE  - path to private key file (default: key.pem)
    #[cfg(feature = "tls")]
    if enable_tls {
        let cert_file = std::env::var("TLS_CERT_FILE").unwrap_or_else(|_| "cert.pem".to_string());
        let key_file = std::env::var("TLS_KEY_FILE").unwrap_or_else(|_| "key.pem".to_string());
        builder = builder.with_tls(cert_file, key_file);
    }

    let config = builder.build();

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
