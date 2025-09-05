//! # 05: Error Handling - Robust Error Management
//!
//! **Learning Goals (15 minutes):**
//! - Use McpError types effectively
//! - Handle errors gracefully
//! - Provide helpful error messages
//! - Implement retry patterns
//!
//! **Prerequisites:** Previous tutorials (01-04)
//!
//! **Run with:** `cargo run --example 05_error_handling`

use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::{Context, McpResult, mcp_error, server, tool};

/// Calculator with comprehensive error handling
#[derive(Clone)]
struct SafeCalculator {
    history: Arc<RwLock<Vec<String>>>,
    max_history: usize,
}

#[server(
    name = "SafeCalculator",
    version = "1.0.0",
    description = "Tutorial 05: Error handling patterns"
)]
impl SafeCalculator {
    fn new() -> Self {
        Self {
            history: Arc::new(RwLock::new(Vec::new())),
            max_history: 100,
        }
    }

    #[tool("Divide two numbers with proper error handling")]
    async fn safe_divide(&self, ctx: Context, dividend: f64, divisor: f64) -> McpResult<f64> {
        ctx.info(&format!("Dividing {} by {}", dividend, divisor))
            .await?;

        // Check for division by zero
        if divisor == 0.0 {
            return Err(mcp_error!("Division by zero is not allowed").into());
        }

        // Check for overflow conditions
        if dividend == f64::MAX && divisor.abs() < 1.0 {
            return Err(mcp_error!("Operation would cause overflow").into());
        }

        let result = dividend / divisor;

        // Check for invalid results
        if result.is_nan() {
            return Err(mcp_error!("Operation resulted in NaN").into());
        }

        if result.is_infinite() {
            return Err(mcp_error!("Operation resulted in infinity").into());
        }

        // Log successful operation
        self.log_operation(format!("{} / {} = {}", dividend, divisor, result))
            .await?;

        Ok(result)
    }

    #[tool("Parse and evaluate expression")]
    async fn evaluate(&self, ctx: Context, expression: String) -> McpResult<f64> {
        ctx.info(&format!("Evaluating expression: {}", expression))
            .await?;

        // Validate input
        if expression.is_empty() {
            return Err(mcp_error!("Expression cannot be empty").into());
        }

        if expression.len() > 1000 {
            return Err(mcp_error!("Expression too long (max 1000 characters)").into());
        }

        // Simple expression parser using basic string operations
        let parts: Vec<&str> = expression.split_whitespace().collect();

        if parts.len() != 3 {
            return Err(mcp_error!("Expression must be: number operator number").into());
        }

        let a = parts[0].parse::<f64>().map_err(|_| -> turbomcp::McpError {
            mcp_error!("'{}' is not a valid number", parts[0]).into()
        })?;

        let b = parts[2].parse::<f64>().map_err(|_| -> turbomcp::McpError {
            mcp_error!("'{}' is not a valid number", parts[2]).into()
        })?;

        match parts[1] {
            "+" => Ok(a + b),
            "-" => Ok(a - b),
            "*" => Ok(a * b),
            "/" => self.safe_divide(ctx, a, b).await,
            op => Err(mcp_error!("Unknown operator: '{}'. Use +, -, *, or /", op).into()),
        }
    }

    #[tool("Get calculation history")]
    async fn get_history(&self, ctx: Context, limit: Option<usize>) -> McpResult<Vec<String>> {
        let history = self.history.read().await;
        let limit = limit.unwrap_or(10);

        ctx.info(&format!("Retrieving last {} history entries", limit))
            .await?;

        if limit == 0 {
            return Err(mcp_error!("Limit must be greater than 0").into());
        }

        let start = history.len().saturating_sub(limit);
        Ok(history[start..].to_vec())
    }

    #[tool("Clear history with confirmation")]
    async fn clear_history(&self, ctx: Context, confirm: bool) -> McpResult<String> {
        if !confirm {
            return Err(mcp_error!("Set confirm=true to clear history").into());
        }

        ctx.warn("Clearing calculation history").await?;

        let mut history = self.history.write().await;
        let count = history.len();
        history.clear();

        Ok(format!("Cleared {} entries", count))
    }

    /// Internal helper that can fail
    async fn log_operation(&self, operation: String) -> McpResult<()> {
        let mut history = self.history.write().await;

        // Check capacity
        if history.len() >= self.max_history {
            // Archive old entries to prevent unbounded growth
            history.remove(0);
        }

        history.push(operation);
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt().with_env_filter("info").init();

    tracing::info!("⚠️ Starting Tutorial 05: Error Handling");
    tracing::info!("This server demonstrates:");
    tracing::info!("  - Validation and input checking");
    tracing::info!("  - Appropriate error types");
    tracing::info!("  - Helpful error messages");
    tracing::info!("  - Recovery patterns");

    let server = SafeCalculator::new();

    // The run_stdio method is generated by the #[server] macro
    server.run_stdio().await?;

    Ok(())
}

// 🎯 **Try it out:**
//
//    Run the server:
//    cargo run --example 05_error_handling
//
//    Test error cases:
//    - Tool: safe_divide { "dividend": 10, "divisor": 0 }
//    - Tool: evaluate { "expression": "10 / 0" }
//    - Tool: evaluate { "expression": "invalid" }
//    - Tool: clear_history { "confirm": false }
//    - Tool: get_history { "limit": 0 }

/* 📝 **Key Concepts:**

**New Error Macro Usage (1.0.3):**
- Use `mcp_error!("message", args).into()` for ergonomic error creation
- Format string support: `mcp_error!("Value {} invalid", value)`
- Automatic type conversion with `.into()`
- In closures, add type annotation: `|_| -> turbomcp::McpError { mcp_error!("...").into() }`

**Error Types:**
- McpError::Tool - Tool execution errors
- McpError::Resource - Resource errors
- McpError::Internal - System errors
- McpError::Protocol - Protocol errors

**Validation Patterns:**
- Check inputs early with mcp_error! macro
- Provide specific error messages with context
- Include recovery hints in error messages
- Validate ranges and formats upfront

**Error Messages:**
- Be specific about what went wrong
- Suggest how to fix it
- Include relevant context (use format args)
- Avoid technical jargon

**Recovery Strategies:**
- Validate before processing
- Use defaults when appropriate
- Implement retry logic
- Log errors for debugging

**Best Practices:**
- Fail fast with clear errors
- Use the right error type
- Include actionable information
- Test error paths thoroughly

**Next Steps:**
- Continue to 06_stateful_server.rs
- Learn state management
- Explore concurrency patterns
*/
