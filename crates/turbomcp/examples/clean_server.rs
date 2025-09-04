//! Clean TurboMCP Server Example - Maximum DX with Zero Lifetime Issues
//!
//! This example shows the clean, ergonomic API that TurboMCP provides
//! with compile-time routing and all transports working out of the box.
//!
//! Run with stdio:
//! ```bash
//! cargo run --example clean_server
//! ```
//!
//! Run with HTTP:
//! ```bash
//! cargo run --example clean_server -- --http
//! ```

use turbomcp::{Context, McpResult, tool};
use turbomcp_protocol::types::{Content, CreateMessageRequest, Role, SamplingMessage, TextContent};
use turbomcp_server::sampling::SamplingExt;

#[derive(Clone, Default)]
pub struct CleanServer;

#[turbomcp::server(
    name = "clean-server",
    version = "1.0.0",
    description = "Clean ergonomic server with compile-time routing"
)]
impl CleanServer {
    pub fn new() -> Self {
        Self
    }

    /// Simple tool that just echoes back
    #[tool("Echo a message back")]
    async fn echo(&self, _ctx: Context, message: String) -> McpResult<String> {
        Ok(format!("Echo: {}", message))
    }

    /// Tool that demonstrates sampling (server→client LLM requests)
    #[tool("Ask the LLM a question via sampling")]
    async fn ask_llm(&self, ctx: Context, question: String) -> McpResult<String> {
        // Create sampling request to send to client
        let request = CreateMessageRequest {
            messages: vec![SamplingMessage {
                role: Role::User,
                content: Content::Text(TextContent {
                    text: question.clone(),
                    annotations: None,
                    meta: None,
                }),
            }],
            model_preferences: None,
            system_prompt: Some("You are a helpful assistant.".to_string()),
            include_context: None,
            temperature: Some(0.7),
            max_tokens: 150,
            stop_sequences: None,
            metadata: None,
        };

        // Use the clean SamplingExt API through Context
        let result = ctx.request.create_message(request).await?;

        // Extract text from response
        let response_text = match result.content {
            Content::Text(text) => text.text,
            _ => "Non-text response".to_string(),
        };

        Ok(format!("Q: {}\nA: {}", question, response_text))
    }

    /// Tool that does some computation
    #[tool("Calculate the factorial of a number")]
    async fn factorial(&self, _ctx: Context, n: u32) -> McpResult<u64> {
        let result = (1..=n as u64).product();
        Ok(result)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    let use_http = args.iter().any(|arg| arg == "--http");

    // Create server
    let server = CleanServer::new();

    // Run with appropriate transport
    if use_http {
        println!("Starting HTTP server on http://127.0.0.1:3000");
        println!("Test with:");
        println!("  curl -X POST http://127.0.0.1:3000/mcp \\");
        println!("    -H 'Content-Type: application/json' \\");
        println!(
            "    -d '{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\",\"params\":{{}}}}'"
        );

        server.run_http("127.0.0.1:3000").await?;
    } else {
        tracing::info!("Starting stdio server");
        server.run_stdio().await?;
    }

    Ok(())
}
