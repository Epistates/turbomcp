//! Perfect MCP Sampling Demo
//!
//! This demonstrates the CORRECT MCP architecture for sampling:
//! 1. Server requests sampling via MCP protocol
//! 2. Client delegates to external LLM MCP servers
//! 3. Perfect compliance + maximum DX

use turbomcp::{Context, Server, tool};
use turbomcp_client::Client;
use turbomcp_protocol::types::{
    Content, CreateMessageRequest, SamplingMessage, Role, TextContent,
};

/// Demo server that requests sampling from clients
#[tool]
async fn ask_llm(
    ctx: Context,
    #[turbomcp::mcp_text("Question to ask the LLM")] question: String,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    println!("🤔 Server wants to ask LLM: {}", question);

    // This is the CORRECT way: server requests sampling from client
    let sampling_request = CreateMessageRequest {
        messages: vec![SamplingMessage {
            role: Role::User,
            content: Content::Text(TextContent {
                text: question,
                annotations: None,
                meta: None,
            }),
        }],
        max_tokens: 100,
        model_preferences: None,
        system_prompt: Some("You are a helpful assistant.".to_string()),
        include_context: None,
        temperature: Some(0.7),
        stop_sequences: None,
        metadata: None,
        _meta: None,
    };

    // Server asks client to handle sampling
    let result = ctx.create_message(sampling_request).await?;

    match result.content {
        Content::Text(text) => Ok(format!("LLM Response: {}", text.text)),
        _ => Err("Expected text response".into()),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("🎯 Perfect MCP Sampling Architecture Demo");
    println!("=====================================");
    println!();
    println!("This demo shows the CORRECT way to do LLM sampling in MCP:");
    println!();
    println!("1. 🖥️  Start this server: `cargo run --example perfect_mcp_sampling_demo`");
    println!("2. 🤖 Start LLM server:  `OPENAI_API_KEY=xxx cargo run --example openai_mcp_server`");
    println!("3. 👤 Start client that connects to both and delegates sampling");
    println!();
    println!("The client receives sampling requests from this server and");
    println!("delegates them to the OpenAI MCP server. Perfect compliance!");
    println!();

    Server::new()
        .add_tool(ask_llm)
        .serve_stdio()
        .await?;

    Ok(())
}