//! Example demonstrating ServerBuilder with STDIO bidirectional support
//!
//! This example shows that the ServerBuilder pattern now properly supports
//! bidirectional communication over STDIO transport, fixing the P0 bug where
//! server-initiated requests (sampling, elicitation, roots, ping) would fail.
//!
//! ## What was fixed
//!
//! Before: `ServerBuilder::new().build().run_stdio()` would fail with:
//! "Handler error: Server request dispatcher not configured for bidirectional communication"
//!
//! After: The dispatcher is automatically configured and bidirectional requests work.
//!
//! ## Running this example
//!
//! ```bash
//! cargo run --package turbomcp-server --example stdio_bidirectional_example
//! ```

use turbomcp_protocol::types::{
    CreateMessageRequest, IncludeContext, Root, SamplingMessage, Role, Content, TextContent,
};
use turbomcp_protocol::RequestContext;
use turbomcp_server::{ServerBuilder, ToolHandler};

/// Example tool that uses sampling (server-initiated request to client)
struct SamplingTool;

#[async_trait::async_trait]
impl ToolHandler for SamplingTool {
    fn name(&self) -> &str {
        "test_sampling"
    }

    fn description(&self) -> &str {
        "Test tool that demonstrates sampling works with ServerBuilder"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "The prompt to send to the client"
                }
            },
            "required": ["prompt"]
        })
    }

    async fn call(
        &self,
        arguments: serde_json::Value,
        ctx: RequestContext,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let prompt = arguments["prompt"]
            .as_str()
            .ok_or("Missing 'prompt' argument")?;

        // This is the CRITICAL test - before the fix, this would fail with:
        // "Handler error: Server request dispatcher not configured for bidirectional communication"
        //
        // After the fix, this works because run_stdio() now configures the dispatcher
        let request = CreateMessageRequest {
            messages: vec![SamplingMessage {
                role: Role::User,
                content: Content::Text(TextContent {
                    text: prompt.to_string(),
                    annotations: None,
                    meta: None,
                }),
                metadata: None,
            }],
            max_tokens: 100,
            model_preferences: None,
            system_prompt: Some("You are a test assistant.".to_string()),
            include_context: Some(IncludeContext::None),
            temperature: Some(0.7),
            stop_sequences: None,
            _meta: None,
        };

        // THIS CALL DEMONSTRATES THE FIX
        match ctx.create_message(request).await {
            Ok(result) => Ok(serde_json::json!({
                "success": true,
                "model": result.model,
                "stop_reason": result.stop_reason,
                "content_preview": format!("{:?}", result.content)
            })),
            Err(e) => Ok(serde_json::json!({
                "success": false,
                "error": format!("Sampling failed: {}", e),
                "note": "If this error is 'Server request dispatcher not configured', the bug still exists!"
            })),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configure server using ServerBuilder (not the macro pattern)
    let mut server = ServerBuilder::new()
        .name("stdio-bidirectional-test")
        .version("1.0.0")
        .root("file:///tmp", Some("Test Root".to_string()))
        .build();

    // Register tool that uses sampling
    server
        .registry_mut()
        .register_tool(SamplingTool)
        .expect("Failed to register tool");

    eprintln!("===========================================");
    eprintln!("STDIO Bidirectional Test Server");
    eprintln!("===========================================");
    eprintln!();
    eprintln!("This server tests the fix for P0 bug:");
    eprintln!("  Before: ServerBuilder.run_stdio() couldn't do bidirectional requests");
    eprintln!("  After:  ServerBuilder.run_stdio() works with sampling/elicitation/etc");
    eprintln!();
    eprintln!("Tool available:");
    eprintln!("  • test_sampling - Tests server→client sampling request");
    eprintln!();
    eprintln!("To test manually:");
    eprintln!("  1. Connect with an MCP client");
    eprintln!("  2. Call test_sampling with: {{\"prompt\": \"What is 2+2?\"}}");
    eprintln!("  3. If sampling request reaches client: ✅ Fix works!");
    eprintln!("  4. If error 'dispatcher not configured': ❌ Bug still exists");
    eprintln!();
    eprintln!("Starting server...");
    eprintln!("===========================================");
    eprintln!();

    // Run with STDIO transport
    // THE FIX: This now properly configures bidirectional support
    server.run_stdio().await?;

    Ok(())
}
