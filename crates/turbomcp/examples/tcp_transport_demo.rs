//! # TCP Transport Demo
//!
//! Demonstrates TCP transport for network communication.
//! The `#[turbomcp::server]` macro automatically generates a `run_tcp()` method.
//!
//! **Run with:**
//! ```bash
//! cargo run --example tcp_transport_demo --features tcp
//! ```
//!
//! **Connect with netcat:**
//! ```bash
//! echo '{"jsonrpc":"2.0","method":"tools/list","id":1}' | nc localhost 8765
//! ```
//!
//! **Or use the basic_client example:**
//! First start this server, then in another terminal run the client configured for TCP.

#[cfg(feature = "tcp")]
use turbomcp::prelude::*;

#[derive(Clone)]
#[cfg(feature = "tcp")]
struct TcpDemoServer;

#[cfg(feature = "tcp")]
#[turbomcp::server(name = "tcp-demo", version = "1.0.0")]
impl TcpDemoServer {
    #[tool("Get server information")]
    async fn info(&self) -> McpResult<String> {
        Ok("TCP Transport Server - accepting connections on port 8765".to_string())
    }

    #[tool("Echo a message back")]
    async fn echo(&self, message: String) -> McpResult<String> {
        Ok(format!("TCP Echo: {}", message))
    }

    #[tool("Add two numbers")]
    async fn add(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a + b)
    }

    #[resource("tcp://status")]
    async fn status(&self) -> McpResult<String> {
        Ok("TCP server is running and healthy".to_string())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("🚀 Starting TCP MCP Server");
    tracing::info!("📡 Listening on tcp://127.0.0.1:8765");
    tracing::info!(
        "💡 Test with: echo '{{\"jsonrpc\":\"2.0\",\"method\":\"tools/list\",\"id\":1}}' | nc localhost 8765"
    );

    TcpDemoServer.run_tcp("127.0.0.1:8765").await?;

    Ok(())
}
