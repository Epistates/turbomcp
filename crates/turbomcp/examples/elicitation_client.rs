//! # Elicitation Client - Provides User Input
//!
//! Simple client that connects to an elicitation server and provides user input.
//!
//! **Terminal 1 (Server):**
//! ```bash
//! cargo run --example elicitation_server
//! ```
//!
//! **Terminal 2 (Client):**
//! ```bash
//! cargo run --example elicitation_client
//! ```

use async_trait::async_trait;
use std::collections::HashMap;
use turbomcp_client::prelude::*;

/// Simple elicitation handler that provides mock user input
#[derive(Debug)]
struct SimpleElicitationHandler;

#[async_trait]
impl ElicitationHandler for SimpleElicitationHandler {
    async fn handle_elicitation(
        &self,
        request: ElicitationRequest,
    ) -> HandlerResult<ElicitationResponse> {
        eprintln!("[Client] Received elicitation request:");
        eprintln!("  Message: {}", request.message());
        eprintln!("  Required fields: {:?}", request.schema().required);

        // Provide mock responses based on schema
        let mut content = HashMap::new();

        for field_name in request.schema().properties.keys() {
            let value = match field_name.as_str() {
                "name" => serde_json::json!("Alice Johnson"),
                "model" => serde_json::json!("claude-3-5-sonnet"),
                "temperature" => serde_json::json!(0.7),
                "maxTokens" => serde_json::json!(1000),
                _ => serde_json::json!("mock_value"),
            };
            content.insert(field_name.clone(), value);
        }

        eprintln!("[Client] Sending response: {:?}", content);

        Ok(ElicitationResponse::accept(content))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Logs to stderr (stdio transport uses stdout for protocol)
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("🚀 Starting elicitation client (mock user input)");

    // Create client with elicitation handler
    let transport = StdioTransport::new();
    let client = ClientBuilder::new()
        .with_elicitation_handler(Arc::new(SimpleElicitationHandler))
        .build(transport)
        .await?;

    // Connect to server
    let init = client.initialize().await?;
    tracing::info!("✅ Connected to: {}", init.server_info.name);

    // List available tools
    let tools = client.list_tools().await?;
    tracing::info!("🛠️  Server has {} tools", tools.len());
    for tool in &tools {
        tracing::info!("  - {}", tool.name);
    }

    // Test 1: Get user name
    tracing::info!("\n📞 Test 1: Calling get_user_name...");
    let result = client.call_tool("get_user_name", None).await?;
    tracing::info!("📝 Result:\n{}", result);

    // Test 2: Configure model
    tracing::info!("\n📞 Test 2: Calling configure_model...");
    let result = client.call_tool("configure_model", None).await?;
    tracing::info!("📝 Result:\n{}", result);

    tracing::info!("\n✅ Demo complete!");

    Ok(())
}
