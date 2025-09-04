//! # 09: Macros - Elegant Server Development with TurboMCP Macros
//!
//! **Learning Goals (10 minutes):**
//! - Use the `#[server]` macro for automatic server setup
//! - Define tools with `#[tool]` attributes
//! - Create resources with `#[resource]` attributes
//! - Generate prompts with `#[prompt]` attributes
//! - Understand how macros simplify MCP development
//!
//! **What this example demonstrates:**
//! - Macro-based server configuration
//! - Automatic handler registration
//! - Type-safe parameter extraction
//! - Clean, declarative API design
//!
//! **Run with:** `cargo run --example 09_macros`
//! **Test with Claude Desktop** by adding to your MCP configuration

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use turbomcp::{McpResult, prompt, resource, server, tool};

/// A calculator server built with TurboMCP macros
#[derive(Clone)]
struct CalculatorServer {
    /// Store calculation history
    #[allow(dead_code)]
    history: Arc<Mutex<Vec<String>>>,
    /// Store variables for later use
    #[allow(dead_code)]
    variables: Arc<Mutex<HashMap<String, f64>>>,
}

#[server(
    name = "MacroCalculator",
    version = "1.0.0",
    description = "A calculator server demonstrating TurboMCP macros"
)]
#[allow(dead_code)]
impl CalculatorServer {
    /// Create a new calculator server
    fn new() -> Self {
        Self {
            history: Arc::new(Mutex::new(Vec::new())),
            variables: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[tool("Add two numbers")]
    async fn add(&self, a: f64, b: f64) -> McpResult<f64> {
        let result = a + b;
        self.log_operation(format!("{a} + {b} = {result}"));
        Ok(result)
    }

    #[tool("Subtract two numbers")]
    async fn subtract(&self, a: f64, b: f64) -> McpResult<f64> {
        let result = a - b;
        self.log_operation(format!("{a} - {b} = {result}"));
        Ok(result)
    }

    #[tool("Multiply two numbers")]
    async fn multiply(&self, a: f64, b: f64) -> McpResult<f64> {
        let result = a * b;
        self.log_operation(format!("{a} * {b} = {result}"));
        Ok(result)
    }

    #[tool("Divide two numbers")]
    async fn divide(&self, a: f64, b: f64) -> McpResult<f64> {
        if b == 0.0 {
            return Err(turbomcp::McpError::Tool(
                "Cannot divide by zero".to_string(),
            ));
        }
        let result = a / b;
        self.log_operation(format!("{a} / {b} = {result}"));
        Ok(result)
    }

    #[tool("Calculate the power of a number")]
    async fn power(&self, base: f64, exponent: f64) -> McpResult<f64> {
        let result = base.powf(exponent);
        self.log_operation(format!("{base} ^ {exponent} = {result}"));
        Ok(result)
    }

    #[tool("Store a value in a variable")]
    async fn store(&self, name: String, value: f64) -> McpResult<String> {
        let mut vars = self.variables.lock().unwrap();
        vars.insert(name.clone(), value);
        self.log_operation(format!("Stored {name} = {value}"));
        Ok(format!("Stored {name} = {value}"))
    }

    #[tool("Retrieve a stored variable")]
    async fn recall(&self, name: String) -> McpResult<f64> {
        let vars = self.variables.lock().unwrap();
        vars.get(&name)
            .copied()
            .ok_or_else(|| turbomcp::McpError::Tool(format!("Variable '{name}' not found")))
    }

    #[tool("Get calculation history")]
    async fn history(&self) -> McpResult<Vec<String>> {
        let history = self.history.lock().unwrap();
        Ok(history.clone())
    }

    #[tool("Clear calculation history")]
    async fn clear_history(&self) -> McpResult<String> {
        let mut history = self.history.lock().unwrap();
        let count = history.len();
        history.clear();
        Ok(format!("Cleared {count} entries from history"))
    }

    #[resource("calc://history")]
    async fn history_resource(&self, _uri: String) -> McpResult<String> {
        let history = self.history.lock().unwrap();
        if history.is_empty() {
            Ok("No calculations yet".to_string())
        } else {
            Ok(history.join("\n"))
        }
    }

    #[resource("calc://variables")]
    async fn variables_resource(&self, _uri: String) -> McpResult<String> {
        let vars = self.variables.lock().unwrap();
        if vars.is_empty() {
            Ok("No variables stored".to_string())
        } else {
            let entries: Vec<String> = vars.iter().map(|(k, v)| format!("{k} = {v}")).collect();
            Ok(entries.join("\n"))
        }
    }

    #[prompt("Generate a math problem")]
    async fn math_problem(
        &self,
        _ctx: turbomcp::RequestContext,
        _args: Option<serde_json::Value>,
    ) -> McpResult<String> {
        Ok("Calculate the following:\n\n1. What is 15 × 7?\n2. If x = 23 and y = 19, what is x + y?\n3. What is 144 ÷ 12?".to_string())
    }

    #[prompt("Explain a math concept")]
    async fn explain_concept(
        &self,
        _ctx: turbomcp::RequestContext,
        args: Option<serde_json::Value>,
    ) -> Result<String, String> {
        let concept = args
            .as_ref()
            .and_then(|v| v.get("concept"))
            .and_then(|v| v.as_str())
            .unwrap_or("addition");

        let explanation = match concept {
            "addition" => "Addition is combining two or more numbers to get their sum.",
            "subtraction" => "Subtraction is finding the difference between two numbers.",
            "multiplication" => "Multiplication is repeated addition of a number.",
            "division" => "Division is splitting a number into equal parts.",
            _ => "Mathematics is the study of numbers, quantities, and shapes.",
        };

        Ok(explanation.to_string())
    }

    /// Helper method to log operations
    fn log_operation(&self, operation: String) {
        let mut history = self.history.lock().unwrap();
        history.push(operation);
        // Keep only last 100 operations
        if history.len() > 100 {
            history.remove(0);
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging (respects RUST_LOG env var)
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()))
        .init();

    tracing::info!("🚀 Starting Macro Calculator Server");
    tracing::info!("This server demonstrates TurboMCP's macro system");

    let server = CalculatorServer::new();

    // The run_stdio method is automatically generated by the #[server] macro
    server.run_stdio().await?;

    Ok(())
}

// 🎯 **Try it out:**
//
//    Run the server:
//    cargo run --example 09_macros
//
//    Then connect with Claude Desktop and try:
//    - Tool: add { "a": 10, "b": 20 }
//    - Tool: store { "name": "result", "value": 30 }
//    - Tool: recall { "name": "result" }
//    - Resource: calc://history
//    - Prompt: math_problem

/* 📝 **Key Concepts:**

**Macro Benefits:**
- Eliminates boilerplate code
- Automatic handler registration
- Type-safe parameter extraction
- Clean, declarative syntax

**Server Macro:**
- Generates run_stdio() method
- Configures server metadata
- Registers all handlers automatically

**Tool Macro:**
- Extracts function parameters as tool inputs
- Generates JSON schema automatically
- Creates handler wrapper functions

**Resource & Prompt Macros:**
- Similar to tools but for different MCP features
- Automatic type conversion and error handling

**Best Practices:**
- Use descriptive names and descriptions
- Keep handler methods focused and simple
- Return Result types for proper error handling
- Use shared state carefully with proper synchronization

**Next Steps:**
- Combine macros with custom handlers
- Build complex multi-feature servers
- Add authentication and authorization

**Next Example:** Continue exploring advanced TurboMCP features!
*/
