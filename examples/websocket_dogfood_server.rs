//! WebSocket Dogfood Server - Debug Initialize Timeout
//!
//! This example creates a minimal WebSocket server to observe the initialize
//! timeout issue reported by the dogfood team.
//!
//! ## Usage
//!
//! ```bash
//! # Run with debug logging to see ALL WebSocket events
//! RUST_LOG=debug,turbomcp_server::runtime::websocket=trace \
//!   cargo run --example websocket_dogfood_server --features websocket
//!
//! # In another terminal, connect with a client
//! ```
//!
//! ## What to Observe
//!
//! Look for these log messages in order:
//! 1. "WebSocket connection from ..." - Connection accepted
//! 2. "Received WebSocket message: X bytes" - Message received
//! 3. "Handling WebSocket request: method=initialize" - Request parsed
//! 4. "Calling handler for method: initialize" - Handler invoked
//! 5. "Handler returned response for method: initialize" - Handler completed
//! 6. "Sending WebSocket response: X bytes" - Response being sent
//! 7. "Response queued successfully" - Response queued to send loop
//!
//! If any of these don't appear, that's where the flow breaks!

use turbomcp::prelude::*;

#[derive(Clone)]
struct DogfoodServer {
    request_count: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

#[server(
    name = "WebSocket Dogfood Server",
    version = "1.0.0",
    description = "Minimal server to debug WebSocket initialize timeout"
)]
impl DogfoodServer {
    fn new() -> Self {
        Self {
            request_count: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    #[tool("Echo a message back")]
    async fn echo(&self, message: String) -> McpResult<String> {
        let count = self
            .request_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        tracing::info!("Echo request #{}: {}", count + 1, message);
        Ok(format!("Echo #{}: {}", count + 1, message))
    }

    #[tool("Add two numbers")]
    async fn add(&self, a: i64, b: i64) -> McpResult<i64> {
        let count = self
            .request_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        tracing::info!("Add request #{}: {} + {}", count + 1, a, b);
        Ok(a + b)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .with_target(true)
        .with_line_number(true)
        .init();

    let server = DogfoodServer::new();

    println!("\n🐕 WebSocket Dogfood Server Starting...");
    println!("   Address: 127.0.0.1:8080");
    println!("   Endpoint: ws://127.0.0.1:8080/ws");
    println!("\n📊 Logging Configuration:");
    println!("   Use RUST_LOG=debug,turbomcp_server::runtime::websocket=trace");
    println!("   to see detailed WebSocket message flow\n");
    println!("🔍 What to Look For:");
    println!("   1. 'Received WebSocket message' - confirms message received");
    println!("   2. 'Handling WebSocket request: method=initialize' - request parsed");
    println!("   3. 'Calling handler' - handler being invoked");
    println!("   4. 'Handler returned response' - handler completed");
    println!("   5. 'Sending WebSocket response' - response being sent");
    println!("\n⏳ If initialize times out, look for which log is MISSING\n");

    server.run_websocket("127.0.0.1:8080").await?;

    Ok(())
}
