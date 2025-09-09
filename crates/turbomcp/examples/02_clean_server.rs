//! # 02: Clean Server - Modern Minimal MCP
//!
//! **Learning Goals (5 minutes):**
//! - Understand the macro-based API for clean server definition
//! - See the minimal code needed for a functional server
//! - Introduction to the #[server] and #[tool] attributes
//!
//! **What this example demonstrates:**
//! - Cleanest possible server implementation
//! - Automatic schema generation from function signatures
//! - Zero boilerplate with maximum functionality
//!
//! **Run with:** `cargo run --example 02_clean_server`

use turbomcp::prelude::*;

/// The simplest possible MCP server using macros
#[derive(Clone)]
struct CleanServer;

#[turbomcp::server(
    name = "clean-server",
    version = "1.0.0",
    description = "Minimal MCP server demonstrating clean architecture"
)]
impl CleanServer {
    /// Get current server time
    #[tool]
    async fn current_time(&self) -> McpResult<String> {
        Ok(chrono::Utc::now().to_rfc3339())
    }

    /// Echo back a message
    #[tool]
    async fn echo(&self, message: String) -> McpResult<String> {
        Ok(format!("Echo: {}", message))
    }

    /// Perform a calculation
    #[tool]
    async fn calculate(&self, operation: String, a: f64, b: f64) -> McpResult<f64> {
        match operation.as_str() {
            "add" => Ok(a + b),
            "subtract" => Ok(a - b),
            "multiply" => Ok(a * b),
            "divide" if b != 0.0 => Ok(a / b),
            "divide" => Err(McpError::Tool("Division by zero".to_string())),
            _ => Err(McpError::Tool(format!("Unknown operation: {}", operation))),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // That's it! The server is ready to run
    CleanServer.run_stdio().await?;
    Ok(())
}
