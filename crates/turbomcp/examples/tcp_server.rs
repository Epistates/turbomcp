//! TCP Transport Server - Minimal Example
//!
//! Demonstrates TCP transport for network communication.
//!
//! **Run:**
//! ```bash
//! cargo run --example tcp_server --features tcp
//! ```
//!
//! **Connect:**
//! ```bash
//! cargo run --example tcp_client --features tcp
//! ```

#[cfg(feature = "tcp")]
use turbomcp::prelude::*;

#[derive(Clone)]
#[cfg(feature = "tcp")]
struct TcpServer;

#[cfg(feature = "tcp")]
#[turbomcp::server(name = "tcp-demo", version = "1.0.0")]
impl TcpServer {
    #[tool("Echo a message")]
    async fn echo(&self, message: String) -> McpResult<String> {
        Ok(format!("TCP Echo: {}", message))
    }

    #[tool("Add two numbers")]
    async fn add(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a + b)
    }
}

#[tokio::main]
#[cfg(feature = "tcp")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stdout)
        .init();

    tracing::info!("🚀 TCP Server listening on 127.0.0.1:8765");

    TcpServer.run_tcp("127.0.0.1:8765").await?;

    Ok(())
}

#[cfg(not(feature = "tcp"))]
fn main() {
    eprintln!(
        "This example requires the 'tcp' feature. Run with: cargo run --example tcp_server --features tcp"
    );
}
