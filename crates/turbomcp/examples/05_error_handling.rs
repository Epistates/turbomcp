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
use turbomcp::{McpError, McpResult, server, tool};

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
    async fn safe_divide(&self, dividend: f64, divisor: f64) -> McpResult<f64> {
        // Check for division by zero
        if divisor == 0.0 {
            return Err(McpError::Tool(
                "Division by zero is not allowed".to_string(),
            ));
        }

        // Check for overflow conditions
        if dividend == f64::MAX && divisor.abs() < 1.0 {
            return Err(McpError::Tool("Operation would cause overflow".to_string()));
        }

        let result = dividend / divisor;

        // Check for invalid results
        if result.is_nan() {
            return Err(McpError::Tool("Operation resulted in NaN".to_string()));
        }

        if result.is_infinite() {
            return Err(McpError::Tool("Operation resulted in infinity".to_string()));
        }

        // Log successful operation
        self.log_operation(format!("{} / {} = {}", dividend, divisor, result))
            .await?;

        Ok(result)
    }

    #[tool("Parse and evaluate expression")]
    async fn evaluate(&self, expression: String) -> McpResult<f64> {
        // Validate input
        if expression.is_empty() {
            return Err(McpError::Tool("Expression cannot be empty".to_string()));
        }

        if expression.len() > 1000 {
            return Err(McpError::Tool(
                "Expression too long (max 1000 characters)".to_string(),
            ));
        }

        // Simple expression parser using basic string operations
        let parts: Vec<&str> = expression.split_whitespace().collect();

        if parts.len() != 3 {
            return Err(McpError::Tool(
                "Expression must be: number operator number".to_string(),
            ));
        }

        let a = parts[0]
            .parse::<f64>()
            .map_err(|_| McpError::Tool(format!("'{}' is not a valid number", parts[0])))?;

        let b = parts[2]
            .parse::<f64>()
            .map_err(|_| McpError::Tool(format!("'{}' is not a valid number", parts[2])))?;

        match parts[1] {
            "+" => Ok(a + b),
            "-" => Ok(a - b),
            "*" => Ok(a * b),
            "/" => self.safe_divide(a, b).await,
            op => Err(McpError::Tool(format!(
                "Unknown operator: '{}'. Use +, -, *, or /",
                op
            ))),
        }
    }

    #[tool("Get calculation history")]
    async fn get_history(&self, limit: Option<usize>) -> McpResult<Vec<String>> {
        let history = self.history.read().await;
        let limit = limit.unwrap_or(10);

        if limit == 0 {
            return Err(McpError::Tool("Limit must be greater than 0".to_string()));
        }

        let start = history.len().saturating_sub(limit);
        Ok(history[start..].to_vec())
    }

    #[tool("Clear history with confirmation")]
    async fn clear_history(&self, confirm: bool) -> McpResult<String> {
        if !confirm {
            return Err(McpError::Tool(
                "Set confirm=true to clear history".to_string(),
            ));
        }

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

**Error Types:**
- McpError::Tool - Tool execution errors
- McpError::Resource - Resource errors
- McpError::Internal - System errors
- McpError::Protocol - Protocol errors

**Validation Patterns:**
- Check inputs early
- Provide specific error messages
- Include recovery hints
- Validate ranges and formats

**Error Messages:**
- Be specific about what went wrong
- Suggest how to fix it
- Include relevant context
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
