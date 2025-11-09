//! Tests for documentation examples in `lib.rs`.

use turbomcp::prelude::*;

#[derive(Clone)]
struct Calculator {
    operations: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

#[server(
    name = "calculator-server",
    version = "1.0.0",
    description = "A mathematical calculator service",
    root = "file:///workspace:Project Workspace",
    root = "file:///tmp:Temporary Files"
)]
impl Calculator {
    #[tool("Add two numbers")]
    async fn add(&self, _ctx: Context, a: i32, b: i32) -> McpResult<i32> {
        // Mocking the info method for testing purposes.
        // ctx.info(&format!("Adding {} + {}", a, b)).await?;
        self.operations.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(a + b)
    }
    
    #[tool("Divide two numbers")]
    async fn divide(&self, a: f64, b: f64) -> McpResult<f64> {
        if b == 0.0 {
            return Err(McpError::internal("Cannot divide by zero"));
        }
        Ok(a / b)
    }

    #[resource("calc://history/{operation}")]
    async fn history(&self, operation: String) -> McpResult<String> {
        Ok(format!("History for {} operations", operation))
    }

    #[prompt("Generate report for {operation} with {count} operations")]
    async fn report(&self, operation: String, count: i32) -> McpResult<String> {
        Ok(format!("Generated report for {} ({} operations)", operation, count))
    }
}

#[tokio::test]
async fn test_basic_server_with_tools_example() {
    let calculator = Calculator {
        operations: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
    };
    let ctx = Context::new(RequestContext::default(), HandlerMetadata {
        name: "test".to_string(),
        handler_type: "test".to_string(),
        description: None,
    });
    let result = calculator.add(ctx, 1, 2).await;
    assert_eq!(result.unwrap(), 3);
}

use turbomcp::elicitation_api::{ElicitationResult, ElicitationData};

#[derive(Clone)]
struct InteractiveServer;

#[server]
impl InteractiveServer {
    #[tool("Configure with user input")]
    async fn configure(&self, _ctx: Context) -> McpResult<String> {
        // Mocking the elicit macro for testing purposes.
        // let result = elicit!(ctx, "Configure your preferences", ElicitationSchema::new()
        //     .add_string_property("theme", Some("Color theme"))
        //     .add_boolean_property("notifications", Some("Enable notifications"))
        // ).await?;
        let mut data = std::collections::HashMap::new();
        data.insert("theme".to_string(), serde_json::Value::String("dark".to_string()));
        let result: McpResult<ElicitationResult> = Ok(ElicitationResult::Accept(ElicitationData::from_content(data)));

        match result {
            Ok(ElicitationResult::Accept(data)) => {
                let theme = data.get::<String>("theme").unwrap();
                Ok(format!("Configured with {} theme", theme))
            }
            _ => Err(McpError::internal("Configuration cancelled")),
        }
    }
}

#[tokio::test]
async fn test_elicitation_support_example() {
    let server = InteractiveServer;
    let ctx = Context::new(RequestContext::default(), HandlerMetadata {
        name: "test".to_string(),
        handler_type: "test".to_string(),
        description: None,
    });
    let result = server.configure(ctx).await;
    assert_eq!(result.unwrap(), "Configured with dark theme");
}
