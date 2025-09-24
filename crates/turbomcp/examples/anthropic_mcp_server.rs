//! Anthropic Claude MCP Server Example
//!
//! Perfect MCP compliance: Claude integration as a separate MCP server
//! that clients can delegate to via standard MCP protocol.

use turbomcp::{mcp_text, resource, Context, tool};
use turbomcp_protocol::types::{
    Content, Role, SamplingMessage,
};

/// Anthropic MCP Server
#[derive(Debug)]
struct AnthropicMcpServer {
    api_key: String,
    client: reqwest::Client,
}

impl AnthropicMcpServer {
    fn new(api_key: String) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("turbomcp-anthropic-server/1.0.0")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self { api_key, client }
    }
}

/// Claude completion via MCP
#[tool]
async fn complete_with_claude(
    ctx: Context,
    #[mcp_text("The messages for completion")] messages: String,
    #[mcp_text("Model to use (default: claude-3-sonnet-20240229)")] model: Option<String>,
    #[mcp_text("System prompt")] system_prompt: Option<String>,
    #[mcp_text("Max tokens")] max_tokens: Option<u32>,
    #[mcp_text("Temperature")] temperature: Option<f64>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let server = ctx.get::<AnthropicMcpServer>().unwrap();

    // Parse messages
    let parsed_messages: Vec<SamplingMessage> = serde_json::from_str(&messages)?;

    // Convert to Anthropic format
    let mut anthropic_messages = vec![];

    for msg in parsed_messages {
        let role = match msg.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };

        let content = match msg.content {
            Content::Text(text) => text.text,
            _ => return Err("Only text content supported".into()),
        };

        anthropic_messages.push(serde_json::json!({
            "role": role,
            "content": content
        }));
    }

    // Build request
    let mut request_body = serde_json::json!({
        "model": model.unwrap_or_else(|| "claude-3-sonnet-20240229".to_string()),
        "messages": anthropic_messages,
        "max_tokens": max_tokens.unwrap_or(1000)
    });

    if let Some(system) = system_prompt {
        request_body["system"] = serde_json::Value::String(system);
    }

    if let Some(temp) = temperature {
        request_body["temperature"] = serde_json::Value::Number(
            serde_json::Number::from_f64(temp).unwrap_or_else(|| serde_json::Number::from(0))
        );
    }

    // Make Anthropic API request
    let response = server
        .client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &server.api_key)
        .header("anthropic-version", "2023-06-01")
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await?;
        return Err(format!("Anthropic API error: {}", error_text).into());
    }

    let response_json: serde_json::Value = response.json().await?;

    let content = response_json["content"][0]["text"]
        .as_str()
        .ok_or("No content in Anthropic response")?;

    Ok(content.to_string())
}

/// Get Claude model info
#[tool]
async fn list_claude_models(
    _ctx: Context,
) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    // Anthropic doesn't have a models endpoint, so return known models
    Ok(vec![
        "claude-3-haiku-20240307".to_string(),
        "claude-3-sonnet-20240229".to_string(),
        "claude-3-opus-20240229".to_string(),
    ])
}

/// Claude capabilities and pricing
#[resource]
async fn claude_info(_ctx: Context) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    Ok(r#"
# Claude Models (Anthropic)

## Claude 3 Model Family

### Claude 3 Haiku
- Fast, cost-effective
- $0.25/1M input tokens, $1.25/1M output tokens
- Good for simple tasks

### Claude 3 Sonnet
- Balanced performance
- $3/1M input tokens, $15/1M output tokens
- Great for most use cases

### Claude 3 Opus
- Most capable model
- $15/1M input tokens, $75/1M output tokens
- Best for complex reasoning

## Capabilities
- 200K token context window
- Vision (images)
- Code generation and analysis
- Complex reasoning and analysis

*Check Anthropic's pricing page for current rates*
"#.trim().to_string())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY environment variable not set")?;

    println!("🧠 Starting Claude MCP Server");
    println!("   Perfect MCP architecture:");
    println!("   - Claude as external MCP server");
    println!("   - Clients delegate via protocol");
    println!("   - Clean separation of concerns\n");

    let anthropic_server = AnthropicMcpServer::new(api_key);

    turbomcp::Server::new()
        .with_context(anthropic_server)
        .add_tool(complete_with_claude)
        .add_tool(list_claude_models)
        .add_resource(claude_info)
        .serve_stdio()
        .await?;

    Ok(())
}