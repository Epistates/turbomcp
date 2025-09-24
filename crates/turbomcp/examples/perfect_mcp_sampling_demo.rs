//! Perfect MCP Sampling Demo
//!
//! This demonstrates the CORRECT MCP architecture for sampling:
//! 1. Server requests sampling via MCP protocol
//! 2. Client delegates to external LLM MCP servers
//! 3. Perfect compliance + maximum DX

use serde::{Deserialize, Serialize};
use turbomcp::prelude::*;

/// Demo server that requests sampling from clients
#[derive(Debug, Clone)]
struct SamplingDemoServer;

/// Parameters for asking the LLM
#[derive(Debug, Deserialize, Serialize)]
struct AskLLMParams {
    /// Question to ask the LLM
    question: String,
}

#[turbomcp::server(name = "SamplingDemo", version = "1.0.0")]
impl SamplingDemoServer {
    #[tool("Ask an LLM a question via MCP sampling")]
    async fn ask_llm(&self, _ctx: Context, params: AskLLMParams) -> McpResult<String> {
        // This demo shows the concept - in practice, servers would use ctx.create_message()
        // to request sampling from clients, but that requires client implementation

        // For demo purposes, we'll show what the server WOULD do:
        let demo_response = format!(
            "Demo: Server would request LLM to answer '{}' via MCP sampling protocol",
            params.question
        );

        Ok(demo_response)
    }
}

#[tokio::main]
async fn main() -> McpResult<()> {
    let server = SamplingDemoServer;

    // Start MCP server - no logging for STDIO protocol
    server
        .run_stdio()
        .await
        .map_err(|e| McpError::internal(format!("Server error: {}", e)))?;

    Ok(())
}
