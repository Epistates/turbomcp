//! TCP-Only Server Example
//!
//! This example demonstrates running a server with ONLY TCP transport,
//! without STDIO. Perfect for dedicated server deployments.
//!
//! Build with:
//! ```bash
//! cargo run --example tcp_only_server --no-default-features --features "internal-deps,tcp"
//! ```

use turbomcp::prelude::*;

#[derive(Clone)]
struct TcpOnlyCalculator {
    port: u16,
}

#[turbomcp::server(name = "tcp-calculator", version = "1.0.0")]
impl TcpOnlyCalculator {
    #[tool("Add two numbers")]
    async fn add(&self, a: i32, b: i32) -> McpResult<i32> {
        Ok(a + b)
    }

    #[tool("Get server info")]
    async fn info(&self) -> McpResult<String> {
        Ok(format!("TCP-only server running on port {}", self.port))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Get port from environment or use default
    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse()
        .unwrap_or(3000);

    let server = TcpOnlyCalculator { port };

    println!("🚀 Starting TCP-only MCP server on port {}", port);
    println!("📡 No STDIO transport - this is a dedicated network server");
    println!("🔗 Connect with: nc localhost {}", port);

    // This server ONLY supports TCP - no run_stdio() method available!
    #[cfg(feature = "tcp")]
    {
        server.run_tcp(format!("0.0.0.0:{}", port)).await?;
    }

    #[cfg(not(feature = "tcp"))]
    {
        eprintln!("❌ TCP feature not enabled! Build with --features 'internal-deps,tcp'");
        std::process::exit(1);
    }

    Ok(())
}
