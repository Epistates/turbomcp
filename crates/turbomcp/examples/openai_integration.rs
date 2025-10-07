//! OpenAI MCP Server Example
//!
//! This demonstrates the CORRECT MCP architecture:
//! - OpenAI integration is a separate MCP SERVER
//! - Clients delegate to it via MCP protocol
//! - Perfect separation of concerns
//! - Maximum composability and DX

use serde::{Deserialize, Serialize};
use turbomcp::prelude::*;

/// OpenAI MCP Server that exposes LLM capabilities via MCP protocol
#[derive(Debug, Clone)]
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

/// Parameters for OpenAI completion
#[derive(Debug, Deserialize, Serialize)]
struct CompletionParams {
    /// The messages for completion in JSON format
    messages: String,
    /// Model to use (default: gpt-4)
    model: Option<String>,
    /// System prompt
    system_prompt: Option<String>,
    /// Max tokens
    max_tokens: Option<u32>,
    /// Temperature
    temperature: Option<f64>,
}

#[turbomcp::server(name = "OpenAI", version = "1.0.0")]
impl OpenAIMcpServer {
    /// LLM completion tool - exposes OpenAI via MCP
    #[tool("Complete messages using OpenAI GPT models")]
    async fn complete_with_gpt(&self, params: CompletionParams) -> McpResult<String> {
        // Parse messages from JSON - simplified approach for demo
        let messages_json: serde_json::Value = serde_json::from_str(&params.messages)
            .map_err(|e| McpError::invalid_request(format!("Invalid messages JSON: {}", e)))?;

        // Convert to OpenAI format
        let mut openai_messages = vec![];

        if let Some(system) = &params.system_prompt {
            openai_messages.push(serde_json::json!({
                "role": "system",
                "content": system
            }));
        }

        // Simplified: assume messages_json is already in OpenAI format for demo
        if let serde_json::Value::Array(messages) = messages_json {
            openai_messages.extend(messages);
        } else {
            openai_messages.push(serde_json::json!({
                "role": "user",
                "content": messages_json.as_str().unwrap_or("Hello")
            }));
        }

        // Make OpenAI request
        let request_body = serde_json::json!({
            "model": params.model.unwrap_or_else(|| "gpt-4".to_string()),
            "messages": openai_messages,
            "max_tokens": params.max_tokens.unwrap_or(1000),
            "temperature": params.temperature.unwrap_or(0.7)
        });

        let response = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
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
                "OpenAI API error: {}",
                error_text
            )));
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| McpError::internal(format!("Failed to parse response: {}", e)))?;

        let content = response_json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| McpError::internal("No content in OpenAI response"))?;

        Ok(content.to_string())
    }

    /// Get available OpenAI models
    #[tool("List available OpenAI models")]
    async fn list_openai_models(&self) -> McpResult<Vec<String>> {
        let response = self
            .client
            .get("https://api.openai.com/v1/models")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| McpError::internal(format!("HTTP request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(McpError::internal("Failed to fetch OpenAI models"));
        }

        let models_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| McpError::internal(format!("Failed to parse response: {}", e)))?;

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
    #[tool("Get OpenAI pricing information")]
    async fn pricing_info(&self) -> McpResult<String> {
        Ok(r#"
# OpenAI Pricing (as of 2025)

## GPT-4 Models
- GPT-4: $30/1M input tokens, $60/1M output tokens
- GPT-4-32k: $60/1M input tokens, $120/1M output tokens

## GPT-3.5 Models
- GPT-3.5-turbo: $1/1M input tokens, $2/1M output tokens

*Prices subject to change. Check OpenAI's official pricing page for current rates.*
"#
        .trim()
        .to_string())
    }
}

#[tokio::main]
async fn main() -> McpResult<()> {
    // Get API key from environment
    let api_key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| McpError::invalid_request("OPENAI_API_KEY environment variable not set"))?;

    // Create OpenAI server instance
    let openai_server = OpenAIMcpServer::new(api_key);

    // Start MCP server with OpenAI tools - no logging for STDIO protocol
    openai_server
        .run_stdio()
        .await
        .map_err(|e| McpError::internal(format!("Server error: {}", e)))?;

    Ok(())
}
