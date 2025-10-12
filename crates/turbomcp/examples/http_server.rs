//! # HTTP Server - Web-Compatible MCP
//!
//! Demonstrates HTTP transport with Server-Sent Events for web browsers.
//!
//! Run with: `cargo run --example http_server`
//! Test with: curl -X POST http://localhost:3000/mcp \
//!   -H "Content-Type: application/json" \
//!   -d '{"jsonrpc":"2.0","method":"tools/list","id":1}'

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
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stdout)
        .init();

    tracing::info!("🌐 Starting HTTP/SSE server");
    tracing::info!("📡 Listening on http://localhost:3000/mcp");

    HttpServer
        .run_http_with_path("127.0.0.1:3000", "/mcp")
        .await?;

    Ok(())
}
