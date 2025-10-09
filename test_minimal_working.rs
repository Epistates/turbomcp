//! Minimal working server (HTTP only)

use turbomcp::prelude::*;

#[derive(Clone)]
struct MinimalServer;

#[turbomcp::server(name = "minimal", version = "1.0.0")]
impl MinimalServer {
    #[tool("Test tool")]
    async fn test(&self) -> McpResult<String> {
        Ok("test".to_string())
    }
}
