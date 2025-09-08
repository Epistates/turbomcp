//! Simple HTTP server on port 8080 for cross-compatibility testing
use turbomcp::{Context, McpResult, tool};

#[derive(Clone, Default)]
pub struct TestServer;

#[turbomcp::server(
    name = "turbomcp-test",
    version = "1.0.0",
    description = "TurboMCP test server for rmcp client compatibility"
)]
impl TestServer {
    pub fn new() -> Self {
        Self
    }

    #[tool("Calculate the sum of two numbers")]
    async fn sum(&self, _ctx: Context, a: i32, b: i32) -> McpResult<String> {
        Ok((a + b).to_string())
    }

    #[tool("Say hello")]
    async fn hello(&self, _ctx: Context, name: String) -> McpResult<String> {
        Ok(format!("Hello, {}! From TurboMCP server", name))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    let server = TestServer::new();

    println!("🚀 Starting TurboMCP HTTP server on http://127.0.0.1:8080");
    println!("Ready for rmcp client connections!");

    server.run_http("127.0.0.1:8080").await?;
    Ok(())
}
