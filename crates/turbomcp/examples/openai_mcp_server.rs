//! OpenAI MCP Server Example
//!
//! This demonstrates the CORRECT MCP architecture:
//! - OpenAI integration is a separate MCP SERVER
//! - Clients delegate to it via MCP protocol
//! - Perfect separation of concerns
//! - Maximum composability and DX

use std::collections::HashMap;
use turbomcp::{mcp_text, resource, Context, tool};
use turbomcp_protocol::types::{
    Content, CreateMessageRequest, CreateMessageResult, Role, SamplingMessage, TextContent,
};

/// OpenAI MCP Server that exposes LLM capabilities via MCP protocol
#[derive(Debug)]
struct OpenAIMcpServer {
    api_key: String,
    client: reqwest::Client,
}

impl OpenAIMcpServer {
    fn new(api_key: String) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("turbomcp-openai-server/1.0.0")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self { api_key, client }
    }
}

/// LLM completion tool - exposes OpenAI via MCP
#[tool]
async fn complete_with_gpt(
    ctx: Context,
    #[mcp_text("The messages for completion")] messages: String,
    #[mcp_text("Model to use (default: gpt-4)")] model: Option<String>,
    #[mcp_text("System prompt")] system_prompt: Option<String>,
    #[mcp_text("Max tokens")] max_tokens: Option<u32>,
    #[mcp_text("Temperature")] temperature: Option<f64>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let server = ctx.get::<OpenAIMcpServer>().unwrap();

    // Parse messages from JSON
    let parsed_messages: Vec<SamplingMessage> = serde_json::from_str(&messages)?;

    // Convert to OpenAI format
    let mut openai_messages = vec![];

    if let Some(system) = system_prompt {
        openai_messages.push(serde_json::json!({
            "role": "system",
            "content": system
        }));
    }

    for msg in parsed_messages {
        let role = match msg.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };

        let content = match msg.content {
            Content::Text(text) => text.text,
            _ => return Err("Only text content supported".into()),
        };

        openai_messages.push(serde_json::json!({
            "role": role,
            "content": content
        }));
    }

    // Make OpenAI request
    let request_body = serde_json::json!({
        "model": model.unwrap_or_else(|| "gpt-4".to_string()),
        "messages": openai_messages,
        "max_tokens": max_tokens.unwrap_or(1000),
        "temperature": temperature.unwrap_or(0.7)
    });

    let response = server
        .client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", server.api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await?;
        return Err(format!("OpenAI API error: {}", error_text).into());
    }

    let response_json: serde_json::Value = response.json().await?;

    let content = response_json["choices"][0]["message"]["content"]
        .as_str()
        .ok_or("No content in OpenAI response")?;

    Ok(content.to_string())
}

/// Get available OpenAI models
#[tool]
async fn list_openai_models(
    ctx: Context,
) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    let server = ctx.get::<OpenAIMcpServer>().unwrap();

    let response = server
        .client
        .get("https://api.openai.com/v1/models")
        .header("Authorization", format!("Bearer {}", server.api_key))
        .send()
        .await?;

    if !response.status().is_success() {
        return Err("Failed to fetch OpenAI models".into());
    }

    let models_json: serde_json::Value = response.json().await?;
    let models = models_json["data"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|m| m["id"].as_str())
        .filter(|id| id.starts_with("gpt-"))
        .map(String::from)
        .collect();

    Ok(models)
}

/// OpenAI pricing information
#[resource]
async fn pricing_info(_ctx: Context) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    Ok(r#"
# OpenAI Pricing (as of 2025)

## GPT-4 Models
- GPT-4: $30/1M input tokens, $60/1M output tokens
- GPT-4-32k: $60/1M input tokens, $120/1M output tokens

## GPT-3.5 Models
- GPT-3.5-turbo: $1/1M input tokens, $2/1M output tokens

*Prices subject to change. Check OpenAI's official pricing page for current rates.*
"#.trim().to_string())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Get API key from environment
    let api_key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| "OPENAI_API_KEY environment variable not set")?;

    println!("🚀 Starting OpenAI MCP Server");
    println!("   This is the CORRECT MCP architecture:");
    println!("   - OpenAI integration is a separate MCP server");
    println!("   - Clients delegate via MCP protocol");
    println!("   - Perfect separation of concerns");
    println!("   - Maximum composability\n");

    // Create OpenAI server instance
    let openai_server = OpenAIMcpServer::new(api_key);

    // Start MCP server with OpenAI tools
    turbomcp::Server::new()
        .with_context(openai_server)
        .add_tool(complete_with_gpt)
        .add_tool(list_openai_models)
        .add_resource(pricing_info)
        .serve_stdio()
        .await?;

    Ok(())
}