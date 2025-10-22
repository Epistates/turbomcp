//! # Tools - Essential Tool Patterns
//!
//! Demonstrates different parameter types, error handling, and validation.
//!
//! Run with: `cargo run --example tools`

use serde::{Deserialize, Serialize};
use turbomcp::prelude::*;

#[derive(Clone)]
struct ToolsServer;

/// Structured parameters for complex operations
#[derive(Debug, Deserialize, Serialize)]
struct MathOp {
    operation: String, // "add", "subtract", "multiply", "divide"
    a: f64,
    b: f64,
    precision: Option<u32>, // Optional with default
}

#[turbomcp::server(name = "tools-demo", version = "1.0.0", transports = ["stdio"])]
impl ToolsServer {
    /// Simple parameters - basic types
    #[tool("Add two numbers")]
    async fn add(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a + b)
    }

    /// Error handling - validation and user-friendly errors
    #[tool("Divide two numbers safely")]
    async fn divide(&self, a: f64, b: f64) -> McpResult<f64> {
        if b == 0.0 {
            return Err(McpError::invalid_request("Division by zero not allowed"));
        }
        Ok(a / b)
    }

    /// Optional parameters with defaults
    #[tool("Round a number to specified precision")]
    async fn round(&self, number: f64, precision: Option<u32>) -> McpResult<f64> {
        let prec = precision.unwrap_or(2);
        let multiplier = 10_f64.powi(prec as i32);
        Ok((number * multiplier).round() / multiplier)
    }

    /// Structured parameters - complex input
    #[tool("Perform a math operation")]
    async fn calc(&self, op: MathOp) -> McpResult<f64> {
        let result = match op.operation.as_str() {
            "add" => op.a + op.b,
            "subtract" => op.a - op.b,
            "multiply" => op.a * op.b,
            "divide" => {
                if op.b == 0.0 {
                    return Err(McpError::invalid_request("Division by zero"));
                }
                op.a / op.b
            }
            _ => {
                return Err(McpError::invalid_request(format!(
                    "Unknown operation: {}",
                    op.operation
                )));
            }
        };

        // Apply optional precision
        if let Some(prec) = op.precision {
            let multiplier = 10_f64.powi(prec as i32);
            Ok((result * multiplier).round() / multiplier)
        } else {
            Ok(result)
        }
    }

    /// Context usage - logging and tracing
    #[tool("Add with logging")]
    async fn add_logged(&self, ctx: Context, a: f64, b: f64) -> McpResult<f64> {
        ctx.info(&format!("Adding {} + {}", a, b)).await?;
        Ok(a + b)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ToolsServer.run_stdio().await?;
    Ok(())
}
