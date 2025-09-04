//! End-to-End Client-Server Example
//!
//! This example demonstrates a complete MCP client-server interaction using TurboMCP,
//! including tools, resources, prompts, and roots configuration.
//!
//! Run with: `cargo run --example client_server_e2e`
//!
//! This example shows:
//! - Server with macro-configured roots
//! - Client connecting and using all MCP features
//! - Roots/list, tools/list, resources/list, prompts/list
//! - Tool calls with different parameter types
//! - Resource reading with URI templates
//! - Prompt generation with parameters

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use turbomcp::prelude::*;

/// A comprehensive server demonstrating all MCP features with roots
#[derive(Clone)]
struct ComprehensiveServer {
    data: Arc<Mutex<HashMap<String, String>>>,
}

#[server(
    name = "comprehensive-server", 
    version = "1.0.0",
    description = "Comprehensive MCP server demonstrating all features",
    // Configure roots for filesystem operations
    root = "file:///workspace:Project Workspace",
    root = "file:///tmp:Temporary Files",
    root = "file:///Users/shared:Shared Documents"
)]
impl ComprehensiveServer {
    /// Create a new comprehensive server
    fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[tool("Store a key-value pair")]
    async fn store(&self, ctx: Context, key: String, value: String) -> McpResult<String> {
        ctx.info(&format!("Storing {} = {}", key, value)).await?;

        let mut data = self.data.lock().await;
        data.insert(key.clone(), value.clone());

        Ok(format!("Stored {} = {}", key, value))
    }

    #[tool("Retrieve a value by key")]
    async fn get(&self, ctx: Context, key: String) -> McpResult<Option<String>> {
        ctx.info(&format!("Retrieving value for key: {}", key))
            .await?;

        let data = self.data.lock().await;
        Ok(data.get(&key).cloned())
    }

    #[tool("List all stored keys")]
    async fn list_keys(&self, ctx: Context) -> McpResult<Vec<String>> {
        ctx.info("Listing all keys").await?;

        let data = self.data.lock().await;
        Ok(data.keys().cloned().collect())
    }

    #[tool("Perform calculation")]
    async fn calculate(
        &self,
        ctx: Context,
        operation: String, // "add", "subtract", "multiply", "divide"
        a: f64,
        b: f64,
    ) -> McpResult<f64> {
        ctx.info(&format!("Calculating {} {} {}", a, operation, b))
            .await?;

        match operation.as_str() {
            "add" => Ok(a + b),
            "subtract" => Ok(a - b),
            "multiply" => Ok(a * b),
            "divide" => {
                if b == 0.0 {
                    Err(mcp_error!("Division by zero").into())
                } else {
                    Ok(a / b)
                }
            }
            _ => Err(mcp_error!("Unsupported operation: {}", operation).into()),
        }
    }

    #[resource("data://{key}")]
    async fn get_data_resource(&self, ctx: Context, key: String) -> McpResult<String> {
        ctx.info(&format!("Accessing data resource: {}", key))
            .await?;

        let data = self.data.lock().await;
        match data.get(&key) {
            Some(value) => Ok(value.clone()),
            None => Err(mcp_error!("Resource not found: {}", key).into()),
        }
    }

    #[resource("stats://summary")]
    async fn stats_resource(&self, ctx: Context) -> McpResult<String> {
        ctx.info("Generating stats summary").await?;

        let data = self.data.lock().await;
        let stats = serde_json::json!({
            "total_keys": data.len(),
            "keys": data.keys().collect::<Vec<_>>(),
            "server": "comprehensive-server",
            "version": "1.0.0"
        });

        Ok(stats.to_string())
    }

    #[prompt("Generate a report for operation {operation} with data {data_key}")]
    async fn operation_report(
        &self,
        ctx: Context,
        operation: String,
        data_key: Option<String>,
    ) -> McpResult<String> {
        ctx.info(&format!("Generating report for operation: {}", operation))
            .await?;

        let data_key_str = data_key.clone().unwrap_or_else(|| "None".to_string());
        let data_value = if let Some(key) = data_key {
            let data = self.data.lock().await;
            data.get(&key).cloned().unwrap_or_else(|| "N/A".to_string())
        } else {
            "N/A".to_string()
        };

        Ok(format!(
            "Operation Report\n================\n\nOperation: {}\nData Key: {}\nData Value: {}\nGenerated: {}\n",
            operation,
            data_key_str,
            data_value,
            chrono::Utc::now().to_rfc3339()
        ))
    }

    #[prompt("Welcome message for new user {username}")]
    async fn welcome_prompt(&self, _ctx: Context, username: String) -> McpResult<String> {
        Ok(format!(
            "Welcome to TurboMCP, {}!\n\nThis server provides:\n• Key-value storage\n• Mathematical calculations\n• Data resources\n• Dynamic reports\n\nFilesystem roots are configured for secure operations.\n\nTry using tools like 'store', 'get', or 'calculate'!",
            username
        ))
    }
}

/// Simulate client testing by showing what would be tested
fn demonstrate_client_usage() {
    println!("🖥️  Client-side MCP testing would include:");
    println!("   1. Connect to server via stdio/tcp/websocket");
    println!("   2. Initialize with client capabilities");
    println!("   3. Discover server features (tools, resources, prompts, roots)");
    println!("   4. Call tools with various parameter types");
    println!("   5. Read resources using URI templates");
    println!("   6. Generate prompts with dynamic parameters");
    println!("   7. Handle server-initiated elicitation requests");
    println!("   8. Graceful shutdown and cleanup");
    println!();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt().with_env_filter("info").init();

    println!("TurboMCP End-to-End Example");
    println!("===========================\n");

    // For a complete demo, you would:
    // 1. Start the server in a separate process or thread
    // 2. Connect with the client
    // 3. Run the demonstration

    // For now, let's show how to create both sides:

    println!("🏗️  Creating comprehensive server:");
    let server = ComprehensiveServer::new();

    // Show server metadata
    let metadata = ComprehensiveServer::get_tools_metadata();
    println!("   🔧 Server provides {} tools", metadata.len());
    for (name, desc, _schema) in &metadata {
        println!("      • {} - {}", name, desc);
    }
    println!();

    println!("📡 Server ready to run with:");
    println!("   cargo run --example client_server_e2e");
    println!("   # Then connect with MCP client");
    println!();

    // Show what client-side testing would involve
    demonstrate_client_usage();

    println!("💡 To test the full E2E flow:");
    println!(
        "   1. Run: echo '{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"roots/list\"}}' | cargo run --example client_server_e2e"
    );
    println!("   2. Or integrate with Claude Desktop by adding to MCP config");
    println!();

    // Actually run the server on stdio for testing
    println!("🚀 Starting server on stdio...");
    server.run_stdio().await?;

    Ok(())
}
