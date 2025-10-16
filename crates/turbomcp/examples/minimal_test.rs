use turbomcp::prelude::*;

#[derive(Clone)]
struct MinimalServer;

#[server(name = "My Test Server", version = "1.0.0")]
impl MinimalServer {
    fn new() -> Self {
        Self
    }

    #[tool("test tool")]
    async fn test_tool(&self) -> McpResult<String> {
        Ok("test".to_string())
    }
}

fn main() {
    let server = MinimalServer::new();
    // Check what create_server produces
    let built = server.create_server().unwrap();
    println!("Server name: {}", built.config().name);
}
