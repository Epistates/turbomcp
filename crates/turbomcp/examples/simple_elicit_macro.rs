//! Simple Example: elicit!() Macro Usage
//!
//! This example demonstrates the elicit!() macro with minimal dependencies.
//! It shows both simple prompts and schema-based elicitation.
//!
//! Run with: cargo run --example simple_elicit_macro

use turbomcp::elicitation_api::{ElicitationResult, boolean_builder, text};
use turbomcp::prelude::*;
use turbomcp_macros::elicit;
use turbomcp_protocol::elicitation::ElicitationSchema;

/// A simple server demonstrating elicitation
#[derive(Clone)]
struct SimpleServer;

#[server(
    name = "simple-elicit-demo",
    version = "1.0.4",
    description = "Demonstrates the elicit!() macro"
)]
impl SimpleServer {
    /// Example 1: Simple yes/no confirmation
    ///
    /// The elicit!() macro makes simple prompts very concise
    #[tool("Deploy application")]
    async fn deploy(&self, ctx: Context, environment: String) -> McpResult<String> {
        // Simple one-line confirmation using the macro
        let result = elicit!(ctx, format!("Deploy to {}?", environment))?;

        match result {
            ElicitationResult::Accept(_) => Ok(format!("✅ Deploying to {}", environment)),
            ElicitationResult::Decline(_) => Ok("❌ Deployment cancelled".to_string()),
            ElicitationResult::Cancel => Ok("🚫 Operation cancelled".to_string()),
        }
    }

    /// Example 2: Using the macro with a schema
    ///
    /// World-class DX: Using improved ergonomic builders
    #[tool("Configure settings")]
    async fn configure(&self, ctx: Context) -> McpResult<String> {
        // Build a simple schema with world-class ergonomics
        let mut schema = ElicitationSchema::new();
        schema.properties.insert(
            "verbose".to_string(),
            boolean_builder()
                .title("Verbose Mode")
                .description("Enable detailed output")
                .build(), // Into conversion!
        );

        // Use the macro with the schema
        let result = elicit!(ctx, "Configure settings", schema)?;

        match result {
            ElicitationResult::Accept(data) => {
                let verbose = data.get_boolean("verbose").unwrap_or(false);
                Ok(format!("Settings configured: verbose={}", verbose))
            }
            _ => Ok("Configuration cancelled".to_string()),
        }
    }

    /// Example 3: Function API for comparison
    ///
    /// For complex schemas, the function API provides more control
    #[tool("Advanced configuration")]
    async fn configure_advanced(&self, ctx: Context) -> McpResult<String> {
        use turbomcp::elicitation_api::elicit as elicit_fn;

        // Function API with world-class ergonomics
        let result = elicit_fn("Advanced Configuration")
            .field(
                "mode",
                text("Operation Mode").options(&["fast", "balanced", "thorough"]), // Beautiful!
            )
            .field("verbose", boolean_builder().title("Verbose Output")) // Zero ceremony!
            .require(vec!["mode"])
            .send(&ctx.request)
            .await?;

        match result {
            ElicitationResult::Accept(data) => {
                let mode = data
                    .get_string("mode")
                    .unwrap_or_else(|_| "balanced".to_string());
                let verbose = data.get_boolean("verbose").unwrap_or(false);
                Ok(format!("Configured: mode={}, verbose={}", mode, verbose))
            }
            _ => Ok("Configuration cancelled".to_string()),
        }
    }

    /// Example 4: Show when to use each approach
    #[tool("Usage guide")]
    async fn guide(&self, _ctx: Context) -> McpResult<String> {
        Ok(r#"
Elicitation API Usage:

1. Use elicit!() MACRO for:
   - Simple yes/no confirmations
   - Basic prompts with minimal fields
   - Quick inline prompts

2. Use elicit() FUNCTION for:
   - Complex multi-field forms
   - Detailed validation rules
   - Reusable configurations

Both are production-ready!"#
            .to_string())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Simple elicit!() Macro Example");
    println!("==============================");
    println!();
    println!("This example shows the elicit!() macro in action.");
    println!();
    println!("Key points:");
    println!("- The macro uses: ctx.request.server_capabilities()");
    println!("- Simple syntax: elicit!(ctx, \"prompt\")?");
    println!("- With schema: elicit!(ctx, \"prompt\", schema)?");
    println!();

    // Create the server
    let _server = SimpleServer;

    println!("✅ Server compiled successfully!");
    println!();
    println!("In a real deployment, this would be run with a transport.");
    println!("The elicit!() macro is now production-ready!");

    Ok(())
}
