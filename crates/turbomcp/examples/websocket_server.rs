//! WebSocket Transport Server - Minimal Example
//!
//! Demonstrates WebSocket transport for real-time bidirectional communication.
//!
//! **Run:**
//! ```bash
//! cargo run --example websocket_server --features "http,websocket"
//! ```
//!
//! **Connect:**
//! ```bash
//! cargo run --example websocket_client --features "http,websocket"
//! ```

#[cfg(feature = "websocket")]
use turbomcp::prelude::*;

#[derive(Clone)]
#[cfg(feature = "websocket")]
struct WebSocketServer;

#[cfg(feature = "websocket")]
#[turbomcp::server(name = "websocket-demo", version = "1.0.0", transports = ["websocket"])]
impl WebSocketServer {
    #[tool("Echo a message")]
    async fn echo(&self, message: String) -> McpResult<String> {
        Ok(format!("WebSocket Echo: {}", message))
    }

    #[tool("Get current timestamp")]
    async fn timestamp(&self) -> McpResult<String> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Ok(format!("Current timestamp: {}", now))
    }

    #[tool("Get WebSocket connection information including headers")]
    async fn connection_info(&self, ctx: Context) -> McpResult<String> {
        let mut info = String::new();

        // Get transport type
        if let Some(transport) = ctx.transport() {
            info.push_str(&format!("Transport: {}\n", transport));
        }

        // Get WebSocket upgrade headers (captured during connection)
        if let Some(headers) = ctx.headers() {
            info.push_str("\nWebSocket Upgrade Headers:\n");
            for (name, value) in headers.iter() {
                info.push_str(&format!("  {}: {}\n", name, value));
            }
        }

        // Get specific headers
        if let Some(user_agent) = ctx.header("user-agent") {
            info.push_str(&format!("\nUser-Agent: {}\n", user_agent));
        }

        if let Some(origin) = ctx.header("origin") {
            info.push_str(&format!("Origin: {}\n", origin));
        }

        // Add request metadata
        info.push_str(&format!("\nRequest ID: {}\n", ctx.request_id()));

        Ok(info)
    }

    #[resource("ws://status")]
    async fn status(&self) -> McpResult<String> {
        Ok("WebSocket server is running".to_string())
    }
}

#[tokio::main]
#[cfg(feature = "websocket")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stdout)
        .init();

    tracing::info!("🚀 WebSocket Server listening on ws://127.0.0.1:8080");

    WebSocketServer.run_websocket("127.0.0.1:8080").await?;

    Ok(())
}

#[cfg(not(feature = "websocket"))]
fn main() {
    eprintln!(
        "This example requires 'http' and 'websocket' features. Run with: cargo run --example websocket_server --features \"http,websocket\""
    );
}
