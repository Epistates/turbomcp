//! WebSocket Transport Server - Minimal Example
//!
//! Demonstrates WebSocket transport for real-time bidirectional communication.
//!
//! **Run:**
//! ```bash
//! cargo run --example websocket_server_simple --features "http,websocket"
//! ```
//!
//! **Connect:**
//! ```bash
//! cargo run --example websocket_client_simple --features "http,websocket"
//! ```

use turbomcp::prelude::*;

#[derive(Clone)]
struct WebSocketServer;

#[turbomcp::server(name = "websocket-demo", version = "1.0.0")]
impl WebSocketServer {
    #[tool("Echo a message")]
    async fn echo(&self, message: String) -> McpResult<String> {
        Ok(format!("WS Echo: {}", message))
    }

    #[tool("Get current timestamp")]
    async fn timestamp(&self) -> McpResult<String> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        Ok(format!("Timestamp: {}", ts))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("🚀 WebSocket Server listening on ws://127.0.0.1:8080");

    WebSocketServer.run_websocket("127.0.0.1:8080").await?;

    Ok(())
}
