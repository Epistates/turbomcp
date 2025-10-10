//! # Unix Socket Transport Demo
//!
//! Demonstrates Unix domain socket transport for fast local IPC.
//! The `#[turbomcp::server]` macro automatically generates a `run_unix()` method.
//!
//! **Platform:** Unix/Linux/macOS only (not available on Windows)
//!
//! **Run with:**
//! ```bash
//! cargo run --example unix_transport_demo --features unix
//! ```
//!
//! **Connect with socat:**
//! ```bash
//! echo '{"jsonrpc":"2.0","method":"tools/list","id":1}' | socat - UNIX-CONNECT:/tmp/turbomcp-demo.sock
//! ```

#[cfg(feature = "unix")]
use turbomcp::prelude::*;

#[derive(Clone)]
#[cfg(feature = "unix")]
struct UnixDemoServer;

#[cfg(feature = "unix")]
#[turbomcp::server(name = "unix-demo", version = "1.0.0")]
impl UnixDemoServer {
    #[tool("Get server information")]
    async fn info(&self) -> McpResult<String> {
        Ok("Unix Socket Transport Server - IPC on /tmp/turbomcp-demo.sock".to_string())
    }

    #[tool("Echo a message via Unix socket")]
    async fn echo(&self, message: String) -> McpResult<String> {
        Ok(format!("Unix IPC Echo: {}", message))
    }

    #[tool("Multiply two numbers")]
    async fn multiply(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a * b)
    }

    #[tool("Get process ID")]
    async fn get_pid(&self) -> McpResult<u32> {
        Ok(std::process::id())
    }

    #[resource("unix://status")]
    async fn status(&self) -> McpResult<String> {
        Ok(format!(
            "Unix socket server running (PID: {})",
            std::process::id()
        ))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    let socket_path = "/tmp/turbomcp-demo.sock";

    // Remove existing socket file if it exists
    if std::path::Path::new(socket_path).exists() {
        tracing::info!("🗑️  Removing existing socket file");
        std::fs::remove_file(socket_path)?;
    }

    tracing::info!("🚀 Starting Unix Socket MCP Server");
    tracing::info!("📡 Creating Unix socket at {}", socket_path);
    tracing::info!(
        "💡 Test with: echo '{{\"jsonrpc\":\"2.0\",\"method\":\"tools/list\",\"id\":1}}' | socat - UNIX-CONNECT:{}",
        socket_path
    );

    UnixDemoServer.run_unix(socket_path).await?;

    Ok(())
}
