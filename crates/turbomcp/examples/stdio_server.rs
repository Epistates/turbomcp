//! Stdio Transport Server - Minimal Example
//!
//! Demonstrates stdio transport for CLI tools and Claude Desktop integration.
//!
//! # ⚠️ CRITICAL: Output Constraint
//!
//! When using stdio transport, **ALL application output must go to stderr**.
//! **Any writes to stdout (including `println!()`) will corrupt the MCP protocol.**
//!
//! ## ✅ Correct Pattern
//!
//! ```ignore
//! // Logs go to stderr (configured below)
//! tracing::info!("message");
//! eprintln!("error message");
//! ```
//!
//! ## ❌ Wrong Pattern
//!
//! ```ignore
//! println!("debug");               // ❌ Corrupts protocol
//! std::io::stdout().write_all(b"x");  // ❌ Corrupts protocol
//! ```
//!
//! The `#[server]` macro with `transports = ["stdio"]` will **reject** any use of
//! `println!()` at compile time, preventing this mistake before deployment.
//!
//! **Run standalone:**
//! ```bash
//! cargo run --example stdio_server --features stdio
//! ```
//!
//! **Test with JSON-RPC:**
//! ```bash
//! echo '{"jsonrpc":"2.0","method":"tools/list","id":1}' | cargo run --example stdio_server
//! ```

use turbomcp::prelude::*;

#[derive(Clone)]
struct StdioServer;

#[turbomcp::server(name = "stdio-demo", version = "1.0.0", transports = ["stdio"])]
impl StdioServer {
    #[tool("Echo a message")]
    async fn echo(&self, message: String) -> McpResult<String> {
        Ok(format!("Stdio Echo: {}", message))
    }

    #[tool("Reverse a string")]
    async fn reverse(&self, text: String) -> McpResult<String> {
        Ok(text.chars().rev().collect())
    }

    #[resource("stdio://status")]
    async fn status(&self) -> McpResult<String> {
        Ok("Stdio server running".to_string())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ⚠️ REQUIRED: Configure logging to stderr for stdio transport
    // Stdio transport uses stdout exclusively for MCP protocol messages.
    // If logging is configured for stdout, it will corrupt the protocol.
    //
    // The #[server(transports = ["stdio"])] macro prevents println!() at compile time,
    // so this logging configuration is the only way to handle application output.
    tracing_subscriber::fmt()
        .with_env_filter("error")
        .with_writer(std::io::stderr) // ← CRITICAL: stderr, not stdout
        .init();

    StdioServer.run_stdio().await?;

    Ok(())
}
