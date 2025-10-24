//! # STDIO Output Verification Example
//!
//! This example demonstrates that:
//! - Protocol messages go to stdout
//! - Observability logs go to stderr
//!
//! Run with output separation:
//! ```bash
//! cargo run --example stdio_output_verification 1>stdout.log 2>stderr.log
//! # Then verify with:
//! cat stdout.log  # Should contain JSON-RPC protocol messages
//! cat stderr.log  # Should contain observability logs
//! ```

use turbomcp::prelude::*;

/// Simple echo app for testing
#[derive(Clone)]
struct EchoApp;

#[turbomcp::server(name = "echo", version = "1.0.0", transports = ["stdio"])]
impl EchoApp {
    #[tool("Echo a message back")]
    async fn echo(&self, message: String) -> McpResult<String> {
        Ok(format!("Echo: {}", message))
    }

    #[tool("Get server status")]
    async fn status(&self) -> McpResult<String> {
        Ok("Server is running".to_string())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize observability (this outputs to stderr by default)
    let _guard = turbomcp_server::observability::ObservabilityConfig::default()
        .with_service_name("stdio-output-verification")
        .enable_security_auditing()
        .enable_performance_monitoring()
        .init()?;

    eprintln!("=== SERVER STARTED: LOGS GO TO STDERR ===");

    // Run the server (protocol messages go to stdout)
    EchoApp.run_stdio().await?;

    Ok(())
}
