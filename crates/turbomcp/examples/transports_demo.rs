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
    // Initialize tracing to stderr (MUST NOT write to stdout as it pollutes the MCP protocol)
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tracing::info!("=== TurboMCP v3 Transports Demonstration ===");

    tracing::info!("In TurboMCP v3, transports are enabled via Cargo features:");
    tracing::info!("  [dependencies]");
    tracing::info!("  turbomcp = {{ version = \"3.0\", features = [\"http\", \"tcp\"] }}");

    tracing::info!("Transport methods available with this build:");
    tracing::info!("  - run_stdio() (always available)");

    #[cfg(feature = "http")]
    tracing::info!("  - run_http(\"0.0.0.0:8080\")");

    #[cfg(feature = "tcp")]
    tracing::info!("  - run_tcp(\"0.0.0.0:9000\")");

    #[cfg(feature = "websocket")]
    tracing::info!("  - run_websocket(\"0.0.0.0:8080\")");

    #[cfg(feature = "unix")]
    tracing::info!("  - run_unix(\"/tmp/mcp.sock\")");

    tracing::info!("=== Usage Examples ===");

    tracing::info!("// STDIO (default, no extra features needed)");
    tracing::info!("TransportsServer.run_stdio().await?;");

    #[cfg(feature = "http")]
    {
        tracing::info!("// HTTP (requires 'http' feature)");
        tracing::info!("TransportsServer.run_http(\"0.0.0.0:8080\").await?;");
    }

    #[cfg(feature = "tcp")]
    {
        tracing::info!("// TCP (requires 'tcp' feature)");
        tracing::info!("TransportsServer.run_tcp(\"0.0.0.0:9000\").await?;");
    }

    tracing::info!("=== Running STDIO server... ===");
    TransportsServer.run_stdio().await?;

    Ok(())
}
