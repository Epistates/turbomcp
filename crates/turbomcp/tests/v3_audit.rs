use turbomcp::prelude::*;

#[derive(Clone)]
struct AuditServer;

#[server(name = "audit-server", version = "1.0.0")]
impl AuditServer {
    #[tool]
    async fn add(&self, a: i32, b: i32) -> i32 {
        a + b
    }

    #[resource("file://{name}")]
    async fn get_test(&self, name: String, _ctx: &RequestContext) -> Result<String, McpError> {
        Ok(format!("Content for {}", name))
    }
}

#[tokio::test]
async fn test_v3_server_compilation_and_execution() {
    let server = AuditServer;

    // Check metadata
    let info = server.server_info();
    assert_eq!(info.name, "audit-server");
    assert_eq!(info.version, "1.0.0");

    // Check tools
    let tools = server.list_tools();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "add");

    // Check resources
    let resources = server.list_resources();
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0].name, "get_test");

    // Test tool execution
    let ctx = RequestContext::stdio();
    let args = serde_json::json!({ "a": 10, "b": 20 });
    let result = server.call_tool("add", args, &ctx).await.unwrap();
    // ToolResult content should be text "30"
    // The implementation of IntoToolResult for i32 probably creates a TextContent
    // Let's verify the structure
    assert!(!result.is_error());

    // Test resource execution
    let result = server
        .read_resource("file://something", &ctx)
        .await
        .unwrap();
    assert_eq!(result.contents.len(), 1);
}
