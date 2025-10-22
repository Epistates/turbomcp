//! # HTTP/SSE Application - Stateful Server
//!
//! A complete working example of an HTTP/SSE server with state management.
//! Demonstrates tools and resources with shared mutable state using Arc + RwLock.
//!
//! ## Quick Start
//!
//! ```bash
//! cargo run --example http_app --features http
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
//! # Increment counter
//! curl -X POST http://localhost:3000/mcp \
//!   -H "Content-Type: application/json" \
//!   -d '{"jsonrpc":"2.0","method":"tools/call","params":{"name":"increment"},"id":2}'
//! ```

use std::sync::Arc;
use tokio::sync::RwLock;

#[cfg(feature = "http")]
use turbomcp::prelude::*;

#[derive(Clone)]
struct WebApp {
    counter: Arc<RwLock<i64>>,
}

#[turbomcp::server(name = "web-app", version = "1.0.0", transports = ["http"])]
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
#[cfg(feature = "http")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stdout)
        .init();

    println!("\n╔══════════════════════════════════════╗");
    println!("║    HTTP/SSE Application Server     ║");
    println!("╚══════════════════════════════════════╝");
    println!("\n🌐 Starting server...");
    println!("📡 Listening on http://localhost:3000/mcp\n");
    println!("Available tools:");
    println!("  • increment - Increment the counter");
    println!("  • get_counter - Get the current counter value\n");
    println!("Available resources:");
    println!("  • app://status - Get application status\n");

    tracing::info!("Starting HTTP/SSE server on 127.0.0.1:3000");

    WebApp::new()
        .run_http_with_path("127.0.0.1:3000", "/mcp")
        .await?;

    Ok(())
}

#[cfg(not(feature = "http"))]
fn main() {
    eprintln!(
        "This example requires the 'http' feature. Run with: cargo run --example http_app --features http"
    );
}
