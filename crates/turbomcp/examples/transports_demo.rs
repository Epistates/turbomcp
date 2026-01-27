//! Demonstration of transport selection in TurboMCP v3.
//!
//! In TurboMCP v3, transport methods are provided by the `McpHandlerExt` trait
//! and are enabled via Cargo features. This is a cleaner approach than the
//! deprecated `transports` attribute.
//!
//! Run with:
//! ```bash
//! cargo run --example transports_demo --features "stdio,http,tcp"
//! ```

use turbomcp::prelude::*;

/// A server that supports all transports enabled via Cargo features.
///
/// In v3, transport methods are available on any `McpHandler` via the
/// `McpHandlerExt` trait when the corresponding feature is enabled:
/// - `run_stdio()` - always available with 'stdio' feature (default)
/// - `run_http(addr)` - requires 'http' feature
/// - `run_tcp(addr)` - requires 'tcp' feature
/// - `run_websocket(addr)` - requires 'websocket' feature
/// - `run_unix(path)` - requires 'unix' feature
#[derive(Clone)]
struct TransportsServer;

#[turbomcp::server(
    name = "transports-demo",
    version = "1.0",
    description = "Demonstrates transport selection in TurboMCP v3"
)]
impl TransportsServer {
    /// A simple tool to demonstrate the server works
    #[tool(description = "Greet someone")]
    async fn greet(&self, name: String) -> McpResult<String> {
        Ok(format!("Hello {} from transports-demo!", name))
    }

    /// Get available transports at runtime
    #[tool(description = "List available transports")]
    async fn list_transports(&self) -> McpResult<Vec<String>> {
        let mut transports = vec!["stdio".to_string()];

        #[cfg(feature = "http")]
        transports.push("http".to_string());

        #[cfg(feature = "tcp")]
        transports.push("tcp".to_string());

        #[cfg(feature = "websocket")]
        transports.push("websocket".to_string());

        #[cfg(feature = "unix")]
        transports.push("unix".to_string());

        Ok(transports)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== TurboMCP v3 Transports Demonstration ===\n");

    println!("In TurboMCP v3, transports are enabled via Cargo features:\n");
    println!("  [dependencies]");
    println!("  turbomcp = {{ version = \"3.0\", features = [\"http\", \"tcp\"] }}\n");

    println!("Transport methods available with this build:\n");
    println!("  - run_stdio() (always available)");

    #[cfg(feature = "http")]
    println!("  - run_http(\"0.0.0.0:8080\")");

    #[cfg(feature = "tcp")]
    println!("  - run_tcp(\"0.0.0.0:9000\")");

    #[cfg(feature = "websocket")]
    println!("  - run_websocket(\"0.0.0.0:8080\")");

    #[cfg(feature = "unix")]
    println!("  - run_unix(\"/tmp/mcp.sock\")");

    println!("\n=== Usage Examples ===\n");

    println!("// STDIO (default, no extra features needed)");
    println!("TransportsServer.run_stdio().await?;\n");

    #[cfg(feature = "http")]
    {
        println!("// HTTP (requires 'http' feature)");
        println!("TransportsServer.run_http(\"0.0.0.0:8080\").await?;\n");
    }

    #[cfg(feature = "tcp")]
    {
        println!("// TCP (requires 'tcp' feature)");
        println!("TransportsServer.run_tcp(\"0.0.0.0:9000\").await?;\n");
    }

    println!("=== Running STDIO server... ===\n");
    TransportsServer.run_stdio().await?;

    Ok(())
}
