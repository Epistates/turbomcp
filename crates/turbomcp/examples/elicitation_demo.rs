//! Elicitation Feature Demo - MCP 2025
//!
//! Demonstrates the elicitation feature for requesting structured user input.
//!
//! Run with:
//! ```bash
//! cargo run --example elicitation_demo
//! ```

use serde_json::json;
use turbomcp::*;
use turbomcp_core::RequestContext;

/// Example server demonstrating elicitation
#[derive(Clone)]
struct ElicitationDemo;

#[server(name = "ElicitationDemo", version = "1.0.0")]
impl ElicitationDemo {
    /// Example tool that uses elicitation internally
    #[tool("Configure application settings with user input")]
    async fn configure_app(&self, ctx: Context) -> McpResult<String> {
        // Log with context
        let _ = ctx.info("Starting configuration with elicitation").await;

        // In a real implementation, this would trigger client-side elicitation
        // The elicitation context would be sent to the client for user input
        let elicitation_schema = json!({
            "type": "object",
            "title": "Application Configuration",
            "required": ["app_name", "port"],
            "properties": {
                "app_name": {
                    "type": "string",
                    "title": "Application Name",
                    "description": "Name of your application",
                    "minLength": 1,
                    "maxLength": 50
                },
                "port": {
                    "type": "integer",
                    "title": "Server Port",
                    "description": "Port number for the server",
                    "minimum": 1024,
                    "maximum": 65535,
                    "default": 8080
                },
                "enable_logging": {
                    "type": "boolean",
                    "title": "Enable Logging",
                    "description": "Enable application logging",
                    "default": true
                },
                "log_level": {
                    "type": "string",
                    "title": "Log Level",
                    "enum": ["debug", "info", "warn", "error"],
                    "default": "info"
                }
            }
        });

        let _ = ctx
            .info(&format!(
                "Elicitation schema defined: {}",
                serde_json::to_string_pretty(&elicitation_schema).unwrap()
            ))
            .await;

        // Simulated user response (in real usage, this comes from the client)
        let user_input = json!({
            "app_name": "MyAwesomeApp",
            "port": 3000,
            "enable_logging": true,
            "log_level": "debug"
        });

        Ok(format!(
            "Configuration received: {}",
            serde_json::to_string_pretty(&user_input).unwrap()
        ))
    }

    /// Example of a completion handler
    #[tool("Get autocomplete suggestions")]
    async fn get_completions(&self, partial: String) -> McpResult<String> {
        let suggestions = match partial.as_str() {
            "app" => vec!["application", "app_name", "app_config"],
            "log" => vec!["log_level", "logging", "logger", "log_file"],
            "por" => vec!["port", "portal", "port_number"],
            _ => vec![],
        };

        Ok(format!("Suggestions for '{}': {:?}", partial, suggestions))
    }

    /// Example of a resource template handler
    #[tool("Get user profile by ID")]
    async fn get_user_profile(&self, user_id: String) -> McpResult<String> {
        // This would handle URI template: /users/{user_id}/profile
        Ok(format!(
            "Profile for user {}: {{name: 'User {}', role: 'developer'}}",
            user_id, user_id
        ))
    }

    /// Example of a ping handler for health checks
    #[tool("Health check")]
    async fn health_check(&self) -> McpResult<String> {
        Ok("Server is healthy and ready for bidirectional communication".to_string())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter("turbomcp=debug,elicitation_demo=debug")
        .init();

    println!("🚀 MCP 2025 Feature Demonstration");
    println!("==================================\n");

    // Create server
    let server = ElicitationDemo;

    // Create context for testing
    let request_ctx = RequestContext::new();
    let handler_metadata = HandlerMetadata {
        name: "test".to_string(),
        description: Some("Test handler".to_string()),
        handler_type: "tool".to_string(),
    };
    let ctx = Context::new(request_ctx, handler_metadata);

    println!("📋 1. ELICITATION DEMO");
    println!("----------------------");
    println!("Elicitation allows servers to request structured input from clients.");
    println!("The server sends a JSON schema, and the client provides validated input.\n");

    match server.configure_app(ctx.clone()).await {
        Ok(result) => println!("✅ {}\n", result),
        Err(e) => println!("❌ Error: {}\n", e),
    }

    println!("🔍 2. COMPLETION DEMO");
    println!("---------------------");
    println!("Completion provides intelligent autocomplete suggestions.\n");

    for partial in &["app", "log", "por", "xyz"] {
        match server.get_completions(partial.to_string()).await {
            Ok(result) => println!("  {}", result),
            Err(e) => println!("  Error: {}", e),
        }
    }

    println!("\n📝 3. RESOURCE TEMPLATE DEMO");
    println!("----------------------------");
    println!("Resource templates use RFC 6570 URI templates for dynamic resources.\n");

    for user_id in &["123", "456", "admin"] {
        match server.get_user_profile(user_id.to_string()).await {
            Ok(result) => println!("  {}", result),
            Err(e) => println!("  Error: {}", e),
        }
    }

    println!("\n❤️ 4. PING/HEALTH CHECK DEMO");
    println!("-----------------------------");
    println!("Bidirectional health monitoring for connection management.\n");

    match server.health_check().await {
        Ok(result) => println!("  {}", result),
        Err(e) => println!("  Error: {}", e),
    }

    println!("\n🎉 MCP 2025 Features Summary:");
    println!("============================");
    println!("✅ Elicitation - Request structured user input with schema validation");
    println!("✅ Completion - Provide intelligent autocomplete suggestions");
    println!("✅ Resource Templates - Dynamic URI templates for parameterized resources");
    println!("✅ Ping Protocol - Bidirectional health monitoring");
    println!("✅ Server-Initiated Requests - Full bidirectional communication");
    println!("\nAll features are now fully integrated into TurboMCP!");

    Ok(())
}
