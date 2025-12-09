//! # Sampling Server - Requests LLM Help
//!
//! Demonstrates how an MCP server can request LLM sampling from the client.
//! The client provides LLM capabilities via the sampling/createMessage request.
//!
//! **Note:** This example shows the server-side pattern. A real client would need
//! LLM integration (OpenAI, Anthropic, etc.) to respond to sampling requests.
//!
//! Run with: `cargo run --example sampling_server`

use std::collections::HashMap;
use turbomcp_protocol::RequestContext;
use turbomcp_protocol::types::{
    CallToolRequest, CallToolResult, Content, CreateMessageRequest, Role, SamplingMessage,
    TextContent, Tool, ToolInputSchema,
};
use turbomcp_server::sampling::SamplingExt;
use turbomcp_server::{ServerBuilder, ServerError, handlers::FunctionToolHandler};

/// Ask the LLM a question via sampling
async fn ask_llm(req: CallToolRequest, ctx: RequestContext) -> Result<CallToolResult, ServerError> {
    // Extract question from tool arguments
    let question = req
        .arguments
        .as_ref()
        .and_then(|args| args.get("question"))
        .and_then(|v| v.as_str())
        .unwrap_or("What is 2+2?");

    // Create sampling request
    let sampling_request = CreateMessageRequest {
        messages: vec![SamplingMessage {
            role: Role::User,
            content: Content::Text(TextContent {
                text: question.to_string(),
                annotations: None,
                meta: None,
            }),
            metadata: None,
        }],
        max_tokens: 500,
        model_preferences: None,
        system_prompt: Some("You are a helpful assistant. Be concise.".to_string()),
        include_context: Some(turbomcp_protocol::types::IncludeContext::None),
        temperature: Some(0.7),
        stop_sequences: None,
        #[cfg(feature = "mcp-sampling-tools")]
        tools: None,
        #[cfg(feature = "mcp-sampling-tools")]
        tool_choice: None,
        task: None,
        _meta: None,
    };

    // Send to client's LLM
    let result = ctx.create_message(sampling_request).await?;

    // Extract response
    let response = match &result.content {
        Content::Text(t) => t.text.clone(),
        _ => "(non-text response)".to_string(),
    };

    Ok(CallToolResult {
        content: vec![Content::Text(TextContent {
            text: format!("Question: {}\n\nLLM Response: {}", question, response),
            annotations: None,
            meta: None,
        })],
        is_error: None,
        structured_content: None,
        _meta: None,
        task_id: None,
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Define tool schema
    let mut properties = HashMap::new();
    properties.insert(
        "question".to_string(),
        serde_json::json!({
            "type": "string",
            "description": "Question to ask the LLM"
        }),
    );

    let ask_tool = Tool {
        name: "ask_llm".to_string(),
        title: Some("Ask LLM".to_string()),
        description: Some("Ask the LLM a question via sampling".to_string()),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: Some(properties),
            required: Some(vec!["question".to_string()]),
            additional_properties: Some(false),
        },
        output_schema: None,
        execution: None,
        annotations: None,
        meta: None,
        #[cfg(feature = "mcp-icons")]
        icons: None,
    };

    // Build and run server
    let server = ServerBuilder::new()
        .name("sampling-demo")
        .version("1.0.0")
        .description("Simple sampling demonstration")
        .tool("ask_llm", FunctionToolHandler::new(ask_tool, ask_llm))?
        .build();

    server.run_stdio().await?;
    Ok(())
}
