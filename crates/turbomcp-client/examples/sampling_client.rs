//! Working MCP Client with Sampling Support
//!
//! This is a real, working client that connects to an MCP server and handles
//! sampling requests from the server.
//!
//! Run the server first in one terminal:
//! ```bash
//! cargo run --example sampling_demo_server
//! ```
//!
//! Then run this client in another terminal to connect:
//! ```bash
//! cargo run --package turbomcp-client --example sampling_client
//! ```

use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use turbomcp_client::Client;
use turbomcp_client::sampling::SamplingHandler;
use turbomcp_protocol::types::{
    Content, CreateMessageRequest, CreateMessageResult, Role, TextContent,
};
use turbomcp_transport::stdio::StdioTransport;

/// Custom sampling handler that provides intelligent responses
#[derive(Debug)]
struct DemoSamplingHandler;

#[async_trait]
impl SamplingHandler for DemoSamplingHandler {
    async fn handle_create_message(
        &self,
        request: CreateMessageRequest,
    ) -> Result<CreateMessageResult, Box<dyn Error + Send + Sync>> {
        eprintln!("[Client] Received sampling request from server");

        // Extract the user's question
        let question = request
            .messages
            .first()
            .and_then(|msg| match &msg.content {
                Content::Text(text) => Some(&text.text),
                _ => None,
            })
            .map(|s| s.as_str())
            .unwrap_or("Unknown question");

        eprintln!("[Client] Question: {}", question);

        // Generate appropriate response based on the question
        let response_text = if question.contains("math") || question.contains("solve") {
            // Math problem response
            if question.contains("2+2") || question.contains("2 + 2") {
                "2 + 2 equals 4. This is basic arithmetic where we add two to two."
            } else if question.contains("factorial") {
                "To calculate a factorial, multiply all positive integers from 1 to n. For example, 5! = 5×4×3×2×1 = 120."
            } else {
                "Let me solve this step by step. First, I identify the operation needed, then apply the mathematical principles to reach the solution."
            }
        } else if question.contains("story") || question.contains("write") {
            // Creative writing response
            if question.contains("dragon") {
                "The ancient dragon stirred in its mountain lair, golden eyes flickering open. A lone knight approached, not with sword drawn, but with an offering of peace."
            } else if question.contains("space") {
                "Stars wheeled overhead as the colony ship drifted through the void. Captain Chen gazed at the distant nebula, humanity's new home waiting beyond."
            } else {
                "Once upon a time, in a land where stories came alive, words danced across pages like butterflies. Each tale brought wonder to those who listened."
            }
        } else if question.contains("code") || question.contains("generate") {
            // Code generation response
            if question.contains("Python") {
                "```python\ndef process_data(items):\n    \"\"\"Process a list of items.\"\"\"\n    return [item.upper() for item in items if item]\n\n# Example usage\nresult = process_data(['hello', 'world'])\nprint(result)  # Output: ['HELLO', 'WORLD']\n```"
            } else if question.contains("JavaScript") {
                "```javascript\nfunction processData(items) {\n    // Filter and transform array\n    return items\n        .filter(item => item)\n        .map(item => item.toUpperCase());\n}\n\n// Example usage\nconst result = processData(['hello', 'world']);\nconsole.log(result); // ['HELLO', 'WORLD']\n```"
            } else if question.contains("Rust") {
                "```rust\nfn process_data(items: Vec<String>) -> Vec<String> {\n    items.into_iter()\n        .filter(|s| !s.is_empty())\n        .map(|s| s.to_uppercase())\n        .collect()\n}\n\n// Example usage\nlet result = process_data(vec![\"hello\".into(), \"world\".into()]);\nprintln!(\"{:?}\", result); // [\"HELLO\", \"WORLD\"]\n```"
            } else {
                "```\nfunction example() {\n    // Implementation here\n    return 'Processed successfully';\n}\n```"
            }
        } else {
            // Generic response
            "I understand your question. Based on the context provided, here's my thoughtful response to help address your query."
        };

        eprintln!(
            "[Client] Sending response: {}",
            &response_text[..50.min(response_text.len())]
        );

        Ok(CreateMessageResult {
            role: Role::Assistant,
            content: Content::Text(TextContent {
                text: response_text.to_string(),
                annotations: None,
                meta: None,
            }),
            model: "demo-llm-v1".to_string(),
            stop_reason: Some("complete".to_string()),
            _meta: None,
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Set up logging to stderr
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter("warn")
        .init();

    eprintln!("🤖 MCP Client with Sampling Support");
    eprintln!("====================================");
    eprintln!();
    eprintln!("This client will:");
    eprintln!("1. Connect to the server via stdio");
    eprintln!("2. Initialize the protocol");
    eprintln!("3. Handle sampling requests from the server");
    eprintln!("4. Call server tools to trigger sampling");
    eprintln!();

    // Create stdio transport for communication
    let transport = StdioTransport::new();

    // Create the client with our custom sampling handler
    let client = Client::new(transport);
    client.set_sampling_handler(Arc::new(DemoSamplingHandler));

    eprintln!("[Client] Initializing protocol...");
    let init_result = client.initialize().await?;
    eprintln!(
        "[Client] Server: {} v{}",
        init_result.server_info.name, init_result.server_info.version
    );

    // List available tools
    eprintln!();
    eprintln!("[Client] Listing available tools...");
    let tools_list = client.list_tools().await?;

    {
        eprintln!("[Client] Available tools:");
        for tool in &tools_list {
            eprintln!(
                "  - {} - {}",
                tool.name,
                tool.description.as_deref().unwrap_or("No description")
            );
        }

        // Demo: Call each tool to trigger sampling
        eprintln!();
        eprintln!("[Client] Demonstrating server->client sampling:");
        eprintln!();

        // Small delay to ensure server is ready
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // 1. Solve a math problem
        eprintln!("=== Testing Math Solver ===");
        let mut args = HashMap::new();
        args.insert("problem".to_string(), json!("What is 2 + 2?"));
        let math_result = client.call_tool("solve_math", Some(args)).await?;

        if let Some(content_array) = math_result.as_array() {
            for content_val in content_array {
                if let Some(text) = content_val.get("text").and_then(|t| t.as_str()) {
                    eprintln!("[Client] Math result:\n{}\n", text);
                }
            }
        }

        // 2. Write a story
        eprintln!("=== Testing Story Writer ===");
        let mut args = HashMap::new();
        args.insert("prompt".to_string(), json!("a brave dragon"));
        let story_result = client.call_tool("write_story", Some(args)).await?;

        if let Some(content_array) = story_result.as_array() {
            for content_val in content_array {
                if let Some(text) = content_val.get("text").and_then(|t| t.as_str()) {
                    eprintln!("[Client] Story result:\n{}\n", text);
                }
            }
        }

        // 3. Generate code
        eprintln!("=== Testing Code Generator ===");
        let mut args = HashMap::new();
        args.insert("language".to_string(), json!("Python"));
        args.insert("task".to_string(), json!("sort a list"));
        let code_result = client.call_tool("generate_code", Some(args)).await?;

        if let Some(content_array) = code_result.as_array() {
            for content_val in content_array {
                if let Some(text) = content_val.get("text").and_then(|t| t.as_str()) {
                    eprintln!("[Client] Code result:\n{}\n", text);
                }
            }
        }
    }

    eprintln!("[Client] Demo complete! All sampling requests handled successfully.");
    eprintln!("[Client] Shutting down...");

    Ok(())
}
