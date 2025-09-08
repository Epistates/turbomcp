//! HTTP server with root "/" endpoint for cross-compatibility testing
use turbomcp::{Context, McpResult, tool};

#[derive(Clone, Default)]
pub struct TestServer;

#[turbomcp::server(
    name = "turbomcp-root-test",
    version = "1.0.0",
    description = "TurboMCP test server with root endpoint"
)]
impl TestServer {
    pub fn new() -> Self {
        Self
    }

    #[tool("Calculate the sum of two numbers")]
    async fn sum(&self, _ctx: Context, a: i32, b: i32) -> McpResult<String> {
        Ok((a + b).to_string())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    let server = TestServer::new();

    println!("🚀 Starting TurboMCP HTTP server on http://127.0.0.1:8080/ (root endpoint)");
    println!(
        "Test with: curl -X POST http://127.0.0.1:8080/ -H 'Content-Type: application/json' -d '{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{{}}}}'"
    );

    server.run_http_with_path("127.0.0.1:8080", "/").await?;
    Ok(())
}
