//! HTTP Transport Server - Minimal Example
//!
//! Demonstrates HTTP/SSE transport for web-based MCP services.
//!
//! **Run:**
//! ```bash
//! cargo run --example http_server_simple --features http
//! ```
//!
//! **Connect:**
//! ```bash
//! cargo run --example http_client --features http
//! ```

#[cfg(feature = "http")]
use turbomcp::prelude::*;

#[cfg(feature = "http")]
#[derive(Clone)]
struct HttpServer;

#[cfg(feature = "http")]
#[turbomcp::server(name = "http-demo", version = "1.0.0")]
impl HttpServer {
    #[tool("Echo a message")]
    async fn echo(&self, message: String) -> McpResult<String> {
        Ok(format!("HTTP Echo: {}", message))
    }

    #[tool("Get server info")]
    async fn info(&self) -> McpResult<String> {
        Ok("HTTP/SSE MCP Server".to_string())
    }

    #[resource("http://data/stats")]
    async fn stats(&self) -> McpResult<String> {
        Ok(r#"{"requests": 42, "uptime": "1h"}"#.to_string())
    }
}

#[tokio::main]
#[cfg(feature = "http")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("🚀 HTTP Server listening on http://127.0.0.1:3000");

    HttpServer.run_http("127.0.0.1:3000").await?;

    Ok(())
}

#[cfg(not(feature = "http"))]
fn main() {
    eprintln!(
        "This example requires the 'http' feature. Run with: cargo run --example http_server_simple --features http"
    );
}
