//! # HTTP Complete Application
//!
//! A complete working example showing HTTP/SSE server with configuration.
//!
//! Run with: `cargo run --example http_app`
//! Test with: curl -X POST http://localhost:3000/mcp \
//!   -H "Content-Type: application/json" \
//!   -d '{"jsonrpc":"2.0","method":"tools/list","id":1}'

#[cfg(feature = "http")]
use std::sync::Arc;
use tokio::sync::RwLock;
#[cfg(feature = "http")]
use turbomcp::prelude::*;

#[derive(Clone)]
struct WebApp {
    counter: Arc<RwLock<i64>>,
}

#[turbomcp::server(name = "web-app", version = "1.0.0")]
impl WebApp {
    fn new() -> Self {
        Self {
            counter: Arc::new(RwLock::new(0)),
        }
    }

    #[tool("Increment counter")]
    async fn increment(&self) -> McpResult<i64> {
        let mut counter = self.counter.write().await;
        *counter += 1;
        Ok(*counter)
    }

    #[tool("Get counter value")]
    async fn get_counter(&self) -> McpResult<i64> {
        let counter = self.counter.read().await;
        Ok(*counter)
    }

    #[resource("app://status")]
    async fn status(&self) -> McpResult<String> {
        let counter = self.counter.read().await;
        Ok(format!("HTTP App - Counter: {}", *counter))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("🌐 Starting HTTP/SSE Application");
    tracing::info!("📡 Listening on http://localhost:3000/mcp");

    WebApp::new()
        .run_http_with_path("127.0.0.1:3000", "/mcp")
        .await?;

    Ok(())
}
