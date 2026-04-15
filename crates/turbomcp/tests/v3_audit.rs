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

// Regression guard: MCP spec 2025-11-25 explicitly permits custom URI schemes,
// so `#[resource("apple-doc://{topic}")]` must dispatch through the macro-
// generated `read_resource` without being rejected by the scheme denylist.
#[derive(Clone)]
struct CustomSchemeServer;

#[server(name = "custom-scheme-server", version = "1.0.0")]
impl CustomSchemeServer {
    #[resource("apple-doc://{topic}")]
    async fn apple_doc(&self, topic: String, _ctx: &RequestContext) -> McpResult<String> {
        Ok(format!("apple-doc content for {topic}"))
    }

    #[resource("notion://{page}")]
    async fn notion(&self, page: String, _ctx: &RequestContext) -> McpResult<String> {
        Ok(format!("notion page {page}"))
    }
}

#[tokio::test]
async fn custom_uri_schemes_reach_registered_handlers() {
    let server = CustomSchemeServer;
    let ctx = RequestContext::stdio();

    let result = server
        .read_resource("apple-doc://swift/StringProtocol", &ctx)
        .await
        .expect("custom apple-doc:// scheme must reach its handler");
    assert_eq!(result.contents.len(), 1);

    let result = server
        .read_resource("notion://workspace-page-abc123", &ctx)
        .await
        .expect("custom notion:// scheme must reach its handler");
    assert_eq!(result.contents.len(), 1);
}

#[tokio::test]
async fn dangerous_uri_schemes_are_still_rejected() {
    let server = CustomSchemeServer;
    let ctx = RequestContext::stdio();

    let err = server
        .read_resource("javascript:alert(1)", &ctx)
        .await
        .expect_err("javascript: scheme must be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("javascript"),
        "error should mention the rejected scheme, got: {msg}"
    );

    let err = server
        .read_resource("vbscript:msgbox(1)", &ctx)
        .await
        .expect_err("vbscript: scheme must be rejected");
    assert!(err.to_string().contains("vbscript"));
}
