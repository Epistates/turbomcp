//! Unix Socket Transport Server - Minimal Example
//!
//! Demonstrates Unix domain socket transport for high-performance local IPC.
//!
//! **Run:**
//! ```bash
//! cargo run --example unix_server --features unix
//! ```
//!
//! **Connect:**
//! ```bash
//! cargo run --example unix_client --features unix
//! ```

#[cfg(feature = "unix")]
use turbomcp::prelude::*;

#[derive(Clone)]
#[cfg(feature = "unix")]
struct UnixServer;

#[cfg(feature = "unix")]
#[turbomcp::server(name = "unix-demo", version = "1.0.0", transports = ["unix"])]
impl UnixServer {
    #[tool("Echo a message")]
    async fn echo(&self, message: String) -> McpResult<String> {
        Ok(format!("Unix Echo: {}", message))
    }

    #[tool("Multiply two numbers")]
    async fn multiply(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a * b)
    }

    #[resource("unix://local/info")]
    async fn info(&self) -> McpResult<String> {
        Ok("Unix socket server - high-performance local IPC".to_string())
    }
}

#[tokio::main]
#[cfg(feature = "unix")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stdout)
        .init();

    let socket_path = "/tmp/turbomcp-demo.sock";

    // Clean up any existing socket
    let _ = std::fs::remove_file(socket_path);

    tracing::info!("🚀 Unix Socket Server listening on {}", socket_path);

    UnixServer.run_unix(socket_path).await?;

    Ok(())
}

#[cfg(not(feature = "unix"))]
fn main() {
    eprintln!(
        "This example requires the 'unix' feature. Run with: cargo run --example unix_server --features unix"
    );
}
