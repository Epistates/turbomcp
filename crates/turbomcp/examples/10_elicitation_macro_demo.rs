//! Example: Working elicit!() Macro Demonstration
//!
//! This example demonstrates the fixed elicit!() macro for simple prompts
//! and the elicit() function for complex schemas.
//!
//! The macro provides concise syntax for simple cases while the function
//! offers maximum control for complex elicitation scenarios.

use turbomcp::elicitation_api::{
    ElicitationResult, boolean_builder, elicit as elicit_fn, integer_builder, string_builder,
};
use turbomcp::prelude::*;
use turbomcp_macros::elicit;
use turbomcp_protocol::elicitation::ElicitationSchema;

#[derive(Clone)]
struct ElicitationServer;

#[server(
    name = "elicitation-macro-demo",
    version = "1.0.4",
    description = "Demonstrates the fixed elicit!() macro"
)]
impl ElicitationServer {
    /// Simple confirmation using the elicit!() macro
    #[tool("Deploy with confirmation")]
    async fn deploy(&self, ctx: Context) -> McpResult<String> {
        // Simple macro usage - clean and concise
        let result = elicit!(ctx, "Deploy to production?")?;

        match result {
            ElicitationResult::Accept(_) => Ok("✅ Deploying to production...".to_string()),
            ElicitationResult::Decline(reason) => {
                Ok(format!("❌ Cancelled: {}", reason.unwrap_or_default()))
            }
            ElicitationResult::Cancel => Ok("🚫 Deployment cancelled".to_string()),
        }
    }

    /// Using the macro with a pre-built schema
    #[tool("Configure with macro")]
    async fn configure_macro(&self, ctx: Context) -> McpResult<String> {
        // Build schema using the builder API
        let mut schema = ElicitationSchema::new();
        schema.properties.insert(
            "debug".to_string(),
            boolean_builder().description("Enable debug mode").build(),
        );

        // Use the macro with schema
        let result = elicit!(ctx, "Configure settings", schema)?;

        match result {
            ElicitationResult::Accept(data) => {
                let debug = data.get_boolean("debug").unwrap_or(false);
                Ok(format!("Debug mode: {}", debug))
            }
            _ => Ok("Configuration cancelled".to_string()),
        }
    }

    /// Complex configuration using the function API
    #[tool("Advanced configuration")]
    async fn configure_advanced(&self, ctx: Context) -> McpResult<String> {
        // Function API for complex schemas - maximum control
        let result = elicit_fn("Database Configuration")
            .field(
                "host",
                string_builder()
                    .title("Database Host")
                    .description("Hostname or IP address")
                    .build(),
            )
            .field(
                "port",
                integer_builder()
                    .title("Port")
                    .range(1024.0, 65535.0)
                    .build(),
            )
            .field("ssl", boolean_builder().title("Enable SSL").build())
            .require(vec!["host"])
            .send(&ctx.request) // Note: send needs RequestContext
            .await?;

        match result {
            ElicitationResult::Accept(data) => {
                let host = data
                    .get_string("host")
                    .unwrap_or_else(|_| "localhost".to_string());
                let port = data.get_integer("port").unwrap_or(5432) as i32;
                let ssl = data.get_boolean("ssl").unwrap_or(true);

                Ok(format!(
                    "✅ Database configured: {}:{} (SSL: {})",
                    host, port, ssl
                ))
            }
            _ => Ok("Configuration cancelled".to_string()),
        }
    }

    /// Demonstrate when to use each approach
    #[tool("Usage guide")]
    async fn usage_guide(&self, _ctx: Context) -> McpResult<String> {
        Ok(r#"
📚 Elicitation API Usage Guide:

🎯 Use elicit!() MACRO when:
• Simple yes/no confirmations
• Basic prompts with minimal fields
• You want concise, readable code
• Working with Context in tool handlers

Example:
  elicit!(ctx, "Continue?")?

🏗️ Use elicit() FUNCTION when:
• Complex multi-field forms
• Rich validation and constraints  
• Detailed field descriptions
• Reusable schema patterns
• Maximum control over schema

Example:
  elicit("Configure")
    .field("port", integer().range(1024.0, 65535.0).build())
    .send(&ctx.request).await?

Both are production-ready and type-safe! 🚀"#
            .to_string())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 TurboMCP elicit!() Macro Demo");
    println!("=================================");
    println!();
    println!("This example demonstrates the FIXED elicit!() macro.");
    println!();
    println!("The macro now correctly uses: ctx.request.server_capabilities()");
    println!("This provides excellent DX for simple elicitation cases!");
    println!();

    let _server = ElicitationServer;

    println!("✅ Server compiled successfully with working elicit!() macro!");
    println!();
    println!("To run this server with a transport:");
    println!("  cargo run --example 10_elicitation_macro_demo");

    Ok(())
}
