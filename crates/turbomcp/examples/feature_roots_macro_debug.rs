//! Debug version to test roots macro configuration
use turbomcp::prelude::*;

#[derive(Clone)]
struct TestServer;

#[server(
    name = "test-server",
    version = "1.0.0",
    root = "file:///tmp:Temp",
    root = "file:///home:Home"
)]
impl TestServer {
    #[tool("Echo test")]
    async fn echo(&self, message: String) -> McpResult<String> {
        Ok(format!("Echo: {}", message))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating server with macro-configured roots...");
    let server = TestServer;

    // The macro should have configured roots via the builder
    println!("Starting server on stdio...");
    server.run_stdio().await?;

    Ok(())
}
