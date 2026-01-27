//! # Tags and Versioning Example
//!
//! Demonstrates how to use tags and versioning for component organization.
//!
//! Tags enable:
//! - Categorizing tools by purpose (admin, readonly, etc.)
//! - Filtering components for access control
//! - Progressive disclosure patterns
//!
//! Versioning enables:
//! - API evolution without breaking changes
//! - Multiple versions of the same tool
//!
//! Run with: `cargo run --example tags_versioning`

use turbomcp::prelude::*;

#[derive(Clone)]
struct TaggedServer;

#[turbomcp::server(name = "tagged-server", version = "1.0.0")]
impl TaggedServer {
    /// A readonly operation - safe to call
    #[tool(description = "Get current timestamp", tags = ["readonly", "public"])]
    async fn get_timestamp(&self) -> McpResult<String> {
        Ok(chrono::Utc::now().to_rfc3339())
    }

    /// An admin-only operation
    #[tool(description = "Delete all data", tags = ["admin", "dangerous"], version = "2.0")]
    async fn delete_all(&self) -> McpResult<String> {
        // In a real server, this would be protected by visibility rules
        Ok("All data deleted (simulated)".into())
    }

    /// V1 of calculate - just adds two numbers
    #[tool(description = "Add two numbers (v1)", tags = ["math"], version = "1.0")]
    async fn calculate_v1(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a + b)
    }

    /// V2 of calculate - supports operation selection
    #[tool(description = "Perform math operation (v2)", tags = ["math"], version = "2.0")]
    async fn calculate_v2(&self, operation: String, a: f64, b: f64) -> McpResult<f64> {
        match operation.as_str() {
            "add" => Ok(a + b),
            "subtract" => Ok(a - b),
            "multiply" => Ok(a * b),
            "divide" if b != 0.0 => Ok(a / b),
            "divide" => Err(McpError::invalid_params("Division by zero")),
            _ => Err(McpError::invalid_params(format!(
                "Unknown operation: {}",
                operation
            ))),
        }
    }

    /// A resource with tags
    #[resource("config://app", tags = ["readonly", "config"])]
    async fn get_config(&self, _uri: String, _ctx: &RequestContext) -> McpResult<String> {
        Ok(r#"{"theme": "dark", "language": "en"}"#.into())
    }

    /// A prompt with tags and version
    #[prompt(description = "Generate greeting", tags = ["greeting"], version = "1.0")]
    async fn greeting_prompt(&self, name: String, ctx: &RequestContext) -> McpResult<PromptResult> {
        let _ = ctx; // Unused in this example
        Ok(PromptResult::user(format!(
            "Generate a friendly greeting for {}",
            name
        )))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Print out the server's tools to show their metadata
    let server = TaggedServer;

    println!("=== Tagged Server Demo ===\n");
    println!("Tools with tags and versions:\n");

    // Access the server info and tools through McpHandler trait
    use turbomcp::__macro_support::turbomcp_core::handler::McpHandler;

    for tool in server.list_tools() {
        println!("Tool: {}", tool.name);
        println!("  Description: {:?}", tool.description);

        if let Some(meta) = &tool.meta {
            if let Some(tags) = meta.get("tags") {
                println!("  Tags: {}", tags);
            }
            if let Some(version) = meta.get("version") {
                println!("  Version: {}", version);
            }
        }
        println!();
    }

    println!("Resources:\n");
    for resource in server.list_resources() {
        println!("Resource: {} ({})", resource.name, resource.uri);
        if let Some(meta) = &resource.meta
            && let Some(tags) = meta.get("tags")
        {
            println!("  Tags: {}", tags);
        }
        println!();
    }

    println!("Prompts:\n");
    for prompt in server.list_prompts() {
        println!("Prompt: {}", prompt.name);
        if let Some(meta) = &prompt.meta {
            if let Some(tags) = meta.get("tags") {
                println!("  Tags: {}", tags);
            }
            if let Some(version) = meta.get("version") {
                println!("  Version: {}", version);
            }
        }
        println!();
    }

    println!("Run with --stdio to start the server:");
    println!("  cargo run --example tags_versioning");
    println!("\nStarting stdio server...");

    TaggedServer.run_stdio().await?;
    Ok(())
}
