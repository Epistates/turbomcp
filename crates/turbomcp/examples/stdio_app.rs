//! # STDIO Complete Application
//!
//! A complete working example showing server and client communication via STDIO.
//!
//! Run with: `cargo run --example stdio_app`

use turbomcp::prelude::*;

/// Complete STDIO application demonstration
#[derive(Clone)]
struct CalculatorApp;

#[turbomcp::server(name = "calculator", version = "1.0.0")]
impl CalculatorApp {
    #[tool("Add two numbers")]
    async fn add(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a + b)
    }

    #[tool("Multiply two numbers")]
    async fn multiply(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a * b)
    }

    #[resource("calc://help")]
    async fn help(&self) -> McpResult<String> {
        Ok("Calculator: add, multiply".to_string())
    }

    #[prompt("Math problem")]
    async fn math_prompt(&self, problem: Option<String>) -> McpResult<String> {
        match problem {
            Some(p) => Ok(format!("Solve: {}", p)),
            None => Ok("Solve this math problem".to_string()),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    CalculatorApp.run_stdio().await?;
    Ok(())
}
