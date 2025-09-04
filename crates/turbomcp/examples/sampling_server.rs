//! Sampling Server - Works with sampling_client
//!
//! This server demonstrates requesting LLM completions from the client
//! through the MCP sampling protocol.
//!
//! Run this server first:
//! ```bash
//! cargo run --example sampling_server
//! ```
//!
//! Then connect the client:
//! ```bash
//! cargo run --example sampling_server 2>/dev/null | \
//!   cargo run --package turbomcp-client --example sampling_client
//! ```

use turbomcp::{Context, McpResult, server, tool};

/// Server that requests LLM completions from the client
#[derive(Clone)]
struct SamplingServer;

#[server(
    name = "sampling-server",
    version = "1.0.0",
    description = "Server that demonstrates sampling requests to client"
)]
impl SamplingServer {
    /// Solve a math problem using the client's LLM
    #[tool("Solve a math problem using LLM")]
    async fn solve_math(&self, _ctx: Context, problem: String) -> McpResult<String> {
        eprintln!("[Server] Received math problem: {}", problem);

        // In a full implementation with sampling support in Context:
        // let request = CreateMessageRequest {
        //     messages: vec![Message { role: Role::User, content: Content::Text(...) }],
        //     ...
        // };
        // let response = ctx.sample(request).await?;
        // return Ok(response.content.to_string());

        // For now, return a message explaining the flow
        Ok(format!(
            "Math Problem: {}\n\n\
            Server would send this to client for LLM processing.\n\
            Client's SamplingHandler processes it and returns the answer.\n\
            See the client terminal for the actual LLM response.",
            problem
        ))
    }

    /// Write a story using the client's LLM
    #[tool("Write a story using LLM")]
    async fn write_story(&self, _ctx: Context, prompt: String) -> McpResult<String> {
        eprintln!("[Server] Received story prompt: {}", prompt);

        Ok(format!(
            "Story Prompt: {}\n\n\
            Server would request a story from client's LLM.\n\
            Client's SamplingHandler generates and returns the story.\n\
            See the client terminal for the generated story.",
            prompt
        ))
    }

    /// Generate code using the client's LLM
    #[tool("Generate code using LLM")]
    async fn generate_code(
        &self,
        _ctx: Context,
        language: String,
        task: String,
    ) -> McpResult<String> {
        eprintln!("[Server] Code generation request: {} in {}", task, language);

        Ok(format!(
            "Code Request: {} in {}\n\n\
            Server would request code generation from client's LLM.\n\
            Client's SamplingHandler generates and returns the code.\n\
            See the client terminal for the generated code.",
            task, language
        ))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("🚀 Sampling Server");
    eprintln!("==================");
    eprintln!();
    eprintln!("This server demonstrates the sampling protocol where");
    eprintln!("the server can request LLM completions from the client.");
    eprintln!();
    eprintln!("Available tools:");
    eprintln!("  • solve_math - Solve math problems using client's LLM");
    eprintln!("  • write_story - Generate stories using client's LLM");
    eprintln!("  • generate_code - Generate code using client's LLM");
    eprintln!();
    eprintln!("Connect sampling_client to see the full flow!");
    eprintln!("Listening on stdio...");

    let server = SamplingServer;
    server.run_stdio().await?;

    Ok(())
}
