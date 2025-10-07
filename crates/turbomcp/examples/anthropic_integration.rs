//! Anthropic Claude MCP Server Example
//!
//! Perfect MCP compliance: Claude integration as a separate MCP server
//! that clients can delegate to via standard MCP protocol.

use serde::{Deserialize, Serialize};
use turbomcp::prelude::*;

/// Anthropic MCP Server
#[derive(Debug, Clone)]
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

/// Parameters for Anthropic completion
#[derive(Debug, Deserialize, Serialize)]
struct ClaudeParams {
    /// The messages for completion in JSON format
    messages: String,
    /// Model to use (default: claude-3-sonnet-20240229)
    model: Option<String>,
    /// System prompt
    system_prompt: Option<String>,
    /// Max tokens
    max_tokens: Option<u32>,
    /// Temperature
    temperature: Option<f64>,
}

#[turbomcp::server(name = "Anthropic", version = "1.0.0")]
impl AnthropicMcpServer {
    /// Claude completion via MCP
    #[tool("Complete messages using Anthropic Claude models")]
    async fn complete_with_claude(&self, params: ClaudeParams) -> McpResult<String> {
        // Parse messages from JSON - simplified approach for demo
        let messages_json: serde_json::Value = serde_json::from_str(&params.messages)
            .map_err(|e| McpError::invalid_request(format!("Invalid messages JSON: {}", e)))?;

        // Convert to Anthropic format
        let mut anthropic_messages = vec![];

        // Simplified: assume messages_json is already in Anthropic format for demo
        if let serde_json::Value::Array(messages) = messages_json {
            anthropic_messages.extend(messages);
        } else {
            anthropic_messages.push(serde_json::json!({
                "role": "user",
                "content": messages_json.as_str().unwrap_or("Hello")
            }));
        }

        // Build request
        let mut request_body = serde_json::json!({
            "model": params.model.unwrap_or_else(|| "claude-3-sonnet-20240229".to_string()),
            "messages": anthropic_messages,
            "max_tokens": params.max_tokens.unwrap_or(1000)
        });

        if let Some(system) = &params.system_prompt {
            request_body["system"] = serde_json::Value::String(system.clone());
        }

        if let Some(temp) = params.temperature {
            request_body["temperature"] = serde_json::Value::Number(
                serde_json::Number::from_f64(temp).unwrap_or_else(|| serde_json::Number::from(0)),
            );
        }

        // Make Anthropic API request
        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| McpError::internal(format!("HTTP request failed: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .map_err(|e| McpError::internal(format!("Failed to read error: {}", e)))?;
            return Err(McpError::internal(format!(
                "Anthropic API error: {}",
                error_text
            )));
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| McpError::internal(format!("Failed to parse response: {}", e)))?;

        let content = response_json["content"][0]["text"]
            .as_str()
            .ok_or_else(|| McpError::internal("No content in Anthropic response"))?;

        Ok(content.to_string())
    }

    /// Get Claude model info
    #[tool("List available Claude models")]
    async fn list_claude_models(&self) -> McpResult<Vec<String>> {
        // Anthropic doesn't have a models endpoint, so return known models
        Ok(vec![
            "claude-3-haiku-20240307".to_string(),
            "claude-3-sonnet-20240229".to_string(),
            "claude-3-opus-20240229".to_string(),
        ])
    }

    /// Claude capabilities and pricing
    #[tool("Get Claude model information and pricing")]
    async fn claude_info(&self) -> McpResult<String> {
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
"#
        .trim()
        .to_string())
    }
}

#[tokio::main]
async fn main() -> McpResult<()> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| McpError::invalid_request("ANTHROPIC_API_KEY environment variable not set"))?;

    let anthropic_server = AnthropicMcpServer::new(api_key);

    // Start MCP server - no logging for STDIO protocol
    anthropic_server
        .run_stdio()
        .await
        .map_err(|e| McpError::internal(format!("Server error: {}", e)))?;

    Ok(())
}
