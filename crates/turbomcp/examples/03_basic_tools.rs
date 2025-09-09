//! # 02: Tools Basics - Essential Tool Patterns
//!
//! **Learning Goals (10 minutes):**
//! - Master different parameter types and patterns
//! - Learn proper error handling in tools
//! - Understand input validation and sanitization
//! - See structured parameters vs simple types
//!
//! **What this example demonstrates:**
//! - Simple parameters (numbers, strings, booleans)
//! - Complex structured parameters with validation
//! - Error handling and user-friendly error messages
//! - Optional parameters with defaults
//! - Context usage for logging and tracing
//!
//! **Run with:** `cargo run --example 02_tools_basics`

use serde::{Deserialize, Serialize};
use turbomcp::prelude::*;

/// Calculator server showing various tool parameter patterns
#[derive(Debug, Clone)]
struct CalculatorServer;

/// Structured parameters for complex operations
#[derive(Debug, Deserialize, Serialize)]
struct MathOperation {
    /// The mathematical operation to perform
    operation: String, // "add", "subtract", "multiply", "divide"
    /// First number
    a: f64,
    /// Second number  
    b: f64,
    /// Optional precision for rounding (default: 2)
    precision: Option<u32>,
}

#[turbomcp::server(name = "Calculator", version = "1.0.0")]
#[allow(dead_code)]
impl CalculatorServer {
    /// Add two numbers together
    ///
    /// Simple tool with basic number parameters and validation
    #[tool("Add two numbers together")]
    async fn add(&self, ctx: Context, a: f64, b: f64) -> McpResult<f64> {
        // Context provides automatic request correlation and structured logging
        ctx.info(&format!("Adding {} + {} with full observability", a, b))
            .await?;

        // Validate inputs to prevent overflow
        if a.is_infinite() || b.is_infinite() {
            ctx.error("Attempted to add infinite numbers").await?;
            return Err(McpError::invalid_request("Cannot add infinite numbers"));
        }

        let result = a + b;

        // Check for overflow
        if result.is_infinite() {
            return Err(McpError::invalid_request(
                "Result overflow - numbers too large",
            ));
        }

        Ok(result)
    }

    /// Safe division with proper error handling
    ///
    /// Demonstrates error handling for edge cases like division by zero
    #[tool("Divide two numbers safely")]
    async fn divide(&self, dividend: f64, divisor: f64) -> McpResult<f64> {
        if divisor == 0.0 {
            return Err(McpError::invalid_request("Division by zero is not allowed"));
        }

        if dividend.is_infinite() || divisor.is_infinite() {
            return Err(McpError::invalid_request("Cannot divide infinite numbers"));
        }

        Ok(dividend / divisor)
    }

    /// Power operation with validation
    ///
    /// Shows validation of reasonable input ranges
    #[tool("Calculate a number raised to a power")]
    async fn power(&self, base: f64, exponent: f64) -> McpResult<f64> {
        // Prevent extremely large calculations
        if !(-1000.0..=1000.0).contains(&exponent) {
            return Err(McpError::invalid_request(
                "Exponent too large (limit: ±1000)",
            ));
        }

        if base.abs() > 1000.0 && exponent > 10.0 {
            return Err(McpError::invalid_request(
                "Base too large for this exponent",
            ));
        }

        let result = base.powf(exponent);

        if result.is_infinite() || result.is_nan() {
            return Err(McpError::invalid_request("Result is not a valid number"));
        }

        Ok(result)
    }

    /// Complex mathematical operation with structured parameters
    ///
    /// Demonstrates complex parameter validation and optional fields
    #[tool("Perform a mathematical operation with structured parameters")]
    async fn calculate(&self, params: MathOperation) -> McpResult<String> {
        // Validate operation type
        let result = match params.operation.to_lowercase().as_str() {
            "add" | "+" => params.a + params.b,
            "subtract" | "-" => params.a - params.b,
            "multiply" | "*" => params.a * params.b,
            "divide" | "/" => {
                if params.b == 0.0 {
                    return Err(McpError::invalid_request("Division by zero"));
                }
                params.a / params.b
            }
            _ => {
                return Err(McpError::invalid_request(format!(
                    "Unknown operation: '{}'. Supported: add, subtract, multiply, divide",
                    params.operation
                )));
            }
        };

        // Check for invalid results
        if result.is_infinite() {
            return Err(McpError::invalid_request("Result is infinite"));
        }
        if result.is_nan() {
            return Err(McpError::invalid_request("Result is not a number"));
        }

        // Apply precision (default to 2 decimal places)
        let precision = params.precision.unwrap_or(2) as usize;
        let formatted_result = format!("{result:.precision$}");

        Ok(format!(
            "{} {} {} = {}",
            params.a, params.operation, params.b, formatted_result
        ))
    }

    /// Format a number with specified precision
    ///
    /// Shows optional parameters with sensible defaults
    #[tool("Format a number with specified decimal places")]
    async fn format_number(&self, number: f64, precision: Option<u32>) -> McpResult<String> {
        let precision = precision.unwrap_or(2);

        // Validate precision is reasonable
        if precision > 10 {
            return Err(McpError::invalid_request(
                "Precision too high (max: 10 decimal places)",
            ));
        }

        Ok(format!("{:.1$}", number, precision as usize))
    }

    /// Check if a number is within a range
    ///
    /// Demonstrates boolean return types and logical operations
    #[tool("Check if a number is within the specified range (inclusive)")]
    async fn is_in_range(&self, number: f64, min: f64, max: f64) -> McpResult<bool> {
        if min > max {
            return Err(McpError::invalid_request(
                "Minimum value cannot be greater than maximum",
            ));
        }

        Ok(number >= min && number <= max)
    }

    /// Get server statistics
    ///
    /// Shows how to return structured information
    #[tool("Get calculator server information and capabilities")]
    async fn info(&self) -> McpResult<String> {
        let info = serde_json::json!({
            "name": "TurboMCP Calculator",
            "version": "1.0.0",
            "features": [
                "Basic arithmetic (add, subtract, multiply, divide)",
                "Power operations",
                "Number formatting",
                "Range validation",
                "Complex structured operations"
            ],
            "parameter_types": {
                "simple": "Direct number/string parameters",
                "structured": "Complex JSON objects with validation",
                "optional": "Parameters with sensible defaults"
            },
            "error_handling": "Comprehensive validation and user-friendly error messages"
        });

        Ok(info.to_string())
    }
}

#[tokio::main]
async fn main() -> McpResult<()> {
    // Enhanced logging to see parameter parsing and validation
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(false)
        .init();

    tracing::info!("🧮 Starting Calculator MCP Server");
    tracing::info!(
        "Available tools: add, divide, power, calculate, format_number, is_in_range, info"
    );
    tracing::info!("Try different parameter types and see error handling in action!");

    let server = CalculatorServer;
    server
        .run_stdio()
        .await
        .map_err(|e| McpError::internal(format!("Server error: {e}")))
}

// 🎯 **Try these examples:**
//
//    Basic operations:
//    - add(5, 3) → 8
//    - divide(10, 3) → 3.333...
//    - divide(10, 0) → Error: Division by zero
//    - power(2, 8) → 256
//
//    Structured operation:
//    - calculate({ "operation": "multiply", "a": 7, "b": 8, "precision": 1 }) → "7 multiply 8 = 56.0"
//
//    Validation examples:
//    - power(999, 999) → Error: Exponent too large
//    - is_in_range(5, 1, 10) → true
//    - format_number(3.14159, 2) → "3.14"

/* 📝 **Key Learning Points:**

   1. **Parameter Types**: Simple (f64, String, bool) vs Complex (structs)
   2. **Error Handling**: Always validate inputs and provide helpful error messages
   3. **Optional Parameters**: Use Option<T> with .unwrap_or() for defaults
   4. **Context Usage**: Log operations for debugging and monitoring
   5. **Input Validation**: Check ranges, prevent overflow, validate operations
   6. **Return Types**: Numbers, strings, booleans, and structured data

   **Next:** `03_macros_vs_builders.rs` - Compare macro vs builder approaches
*/
