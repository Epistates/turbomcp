//! Minimal test case for WebSocket macro bug

use turbomcp::prelude::*;

#[derive(Clone)]
struct MinimalServer;

#[turbomcp::server(name = "minimal", version = "1.0.0")]
impl MinimalServer {
    #[tool("Test tool")]
    async fn test(&self) -> McpResult<String> {
        Ok("test".to_string())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    MinimalServer.run_websocket("127.0.0.1:8080").await
}
