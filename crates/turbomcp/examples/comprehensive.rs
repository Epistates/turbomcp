//! # Comprehensive Client - All MCP Features
//!
//! Demonstrates using all MCP client capabilities.
//!
//! Run with: `cargo run --example comprehensive`

use std::collections::HashMap;
use turbomcp_client::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("🚀 Comprehensive MCP Client Demo");

    let transport = StdioTransport::new();
    let client = Client::new(transport);

    // 1. Initialize
    tracing::info!("📡 Initializing...");
    let init_result = client.initialize().await?;
    tracing::info!(
        "✅ Server: {} v{}",
        init_result.server_info.name,
        init_result.server_info.version
    );

    // 2. List Tools
    tracing::info!("🔍 Listing tools...");
    let tools = client.list_tools().await?;
    for tool in &tools {
        tracing::info!(
            "  🛠️  {}: {}",
            tool.name,
            tool.description.as_deref().unwrap_or("No description")
        );
    }

    // 3. Call Tools
    tracing::info!("📞 Calling tools...");
    let mut args = HashMap::new();
    args.insert("a".to_string(), serde_json::json!(10.0));
    args.insert("b".to_string(), serde_json::json!(5.0));
    let result = client.call_tool("add", Some(args)).await?;
    tracing::info!("  ➕ add(10, 5) = {:?}", result);

    // 4. List Resources
    tracing::info!("📁 Listing resources...");
    let resources = client.list_resources().await?;
    for uri in &resources {
        tracing::info!("  📄 {}", uri);
    }

    // 5. Read Resources
    if !resources.is_empty() {
        tracing::info!("📖 Reading first resource...");
        let content = client.read_resource(&resources[0]).await?;
        tracing::info!("  Content: {:?}", content.contents.first());
    }

    // 6. List Prompts
    tracing::info!("💬 Listing prompts...");
    let prompts = client.list_prompts().await?;
    for prompt in &prompts {
        tracing::info!("  💭 {}", prompt.name);
    }

    // 7. Get Prompt
    if !prompts.is_empty() {
        tracing::info!("🎯 Getting first prompt...");
        let prompt_result = client.get_prompt(&prompts[0].name, None).await?;
        tracing::info!("  Result: {:?}", prompt_result);
    }

    tracing::info!("✨ Comprehensive demo completed!");
    Ok(())
}
