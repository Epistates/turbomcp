//! Serving `2025-06-18` end to end: the handshake echoes it, the session is
//! dispatched in its wire shapes, and the features it predates are neither
//! advertised nor reachable.
//!
//! The same server object serves `2025-11-25` at the same time, so each test
//! that asserts something is *absent* on `2025-06-18` also asserts it is
//! present on `2025-11-25`. Without that control the tests would pass equally
//! well against a server that had simply lost the feature.

use serde_json::{Value, json};
use tower::{Service, ServiceExt};
use turbomcp_core::{Implementation, JsonRpcMessage, JsonRpcRequest, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, GetPromptContext, LegacySessionAdapter, ListPromptsContext,
    ListResourcesContext, ListToolsContext, McpServerCore, ServerBuilder, VersionDispatcher,
    WithPrompts, WithResources, WithTools,
};

/// Everything on this server carries the fields `2025-11-25` added.
#[derive(Clone)]
struct Rich;

impl McpServerCore for Rich {
    fn server_info(&self) -> Implementation {
        Implementation::new("rich", "1.0.0")
    }
}

fn icon() -> neutral::Icon {
    neutral::Icon::new("https://example.com/icon.png")
}

impl WithTools for Rich {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![
            neutral::Tool::new("search", json!({ "type": "object", "properties": {} }))
                .with_title("Search")
                .with_description("Find things")
                .with_icon(icon())
                .with_task_support(neutral::TaskSupport::Optional)
                .with_annotations(neutral::ToolAnnotations::new().read_only()),
        ]))
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        Ok(neutral::CallToolResult::text("ok"))
    }
}

impl WithResources for Rich {
    async fn list_resources(
        &self,
        _ctx: &ListResourcesContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListResourcesResult> {
        Ok(neutral::ListResourcesResult::new(vec![
            neutral::Resource::new("mem://a", "a")
                .with_title("A")
                .with_icon(icon()),
        ]))
    }

    async fn read_resource(
        &self,
        _ctx: &turbomcp_server::ReadResourceContext,
        params: neutral::ReadResourceParams,
    ) -> McpResult<neutral::ReadResourceResult> {
        Ok(neutral::ReadResourceResult::text(params.uri, "body"))
    }
}

impl WithPrompts for Rich {
    async fn list_prompts(
        &self,
        _ctx: &ListPromptsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListPromptsResult> {
        Ok(neutral::ListPromptsResult::new(vec![
            neutral::Prompt::new("greet").with_icon(icon()),
        ]))
    }

    async fn get_prompt(
        &self,
        _ctx: &GetPromptContext,
        _params: neutral::GetPromptParams,
    ) -> McpResult<neutral::GetPromptResult> {
        Ok(neutral::GetPromptResult::new(vec![
            neutral::PromptMessage::user_text("hi"),
        ]))
    }
}

type Svc = LegacySessionAdapter<VersionDispatcher<Rich>>;

/// A fresh connection to a Tasks-enabled server. Tasks are on so the tests can
/// show `2025-06-18` is gated by the *negotiated version*, not by the server
/// simply not having them.
fn connection() -> Svc {
    LegacySessionAdapter::new(
        ServerBuilder::new(Rich)
            .with_tools()
            .with_resources()
            .with_prompts()
            .with_logging()
            .with_tasks()
            .build(),
    )
}

async fn raw(svc: &mut Svc, req: JsonRpcRequest) -> turbomcp_core::JsonRpcResponse {
    match svc
        .ready()
        .await
        .expect("ready")
        .call(req.into())
        .await
        .expect("call")
    {
        Some(JsonRpcMessage::Response(r)) => r,
        other => panic!("expected a response, got {other:?}"),
    }
}

async fn ok(svc: &mut Svc, req: JsonRpcRequest) -> Value {
    let r = raw(svc, req).await;
    assert!(r.error.is_none(), "unexpected error: {:?}", r.error);
    r.result.expect("a result")
}

/// Handshake at `version`, returning the connection and the `initialize`
/// result. Later requests carry no version of their own — the adapter stamps
/// what was negotiated, which is exactly the behavior under test.
async fn connect(version: &str) -> (Svc, Value) {
    let mut svc = connection();
    let init = ok(
        &mut svc,
        JsonRpcRequest::new(
            0,
            "initialize",
            Some(json!({
                "protocolVersion": version,
                "capabilities": {},
                "clientInfo": { "name": "c", "version": "1" },
            })),
        ),
    )
    .await;
    (svc, init)
}

/// The regression this whole revision exists to fix: a `2025-06-18` client used
/// to be answered `2025-11-25`, and the lifecycle spec tells a client that
/// receives a version it can't speak to disconnect. So the handshake must echo.
#[tokio::test]
async fn the_handshake_echoes_2025_06_18() {
    let (_svc, init) = connect("2025-06-18").await;
    assert_eq!(init["protocolVersion"], "2025-06-18", "{init}");
    assert_eq!(init["serverInfo"]["name"], "rich");
}

/// Tasks are `2025-11-25`-only. Advertising them to a `2025-06-18` client
/// would invite `tasks/*` calls the dispatcher can only refuse.
#[tokio::test]
async fn tasks_are_neither_advertised_nor_reachable_on_2025_06_18() {
    let (mut svc, init) = connect("2025-06-18").await;
    assert!(
        init["capabilities"].get("tasks").is_none(),
        "the server has Tasks enabled, but this revision predates them: {init}"
    );
    // The capabilities it *does* have are unaffected.
    assert_eq!(init["capabilities"]["tools"]["listChanged"], true);
    assert_eq!(init["capabilities"]["logging"], json!({}));

    for method in ["tasks/list", "tasks/get", "tasks/cancel", "tasks/result"] {
        let r = raw(&mut svc, JsonRpcRequest::new(1, method, Some(json!({})))).await;
        assert_eq!(
            r.error.as_ref().map(|e| e.code),
            Some(-32601),
            "{method} must be method-not-found on 2025-06-18"
        );
    }

    // The control: the same server does advertise and serve Tasks to a
    // 2025-11-25 client.
    let (_svc, init) = connect("2025-11-25").await;
    assert_eq!(init["capabilities"]["tasks"]["list"], json!({}));
}

/// `icons` and `execution` are `2025-11-25` additions. The handlers return
/// them on every listing; the wire this revision speaks has no field for them.
#[tokio::test]
async fn listings_shed_the_fields_the_revision_does_not_have() {
    let (mut svc, _) = connect("2025-06-18").await;

    let tools = ok(&mut svc, JsonRpcRequest::new(1, "tools/list", None)).await;
    let tool = &tools["tools"][0];
    assert!(tool.get("icons").is_none(), "{tool}");
    assert!(tool.get("execution").is_none(), "{tool}");
    // …and keeps everything the revision does have.
    assert_eq!(tool["name"], "search");
    assert_eq!(tool["title"], "Search");
    assert_eq!(tool["description"], "Find things");
    assert_eq!(tool["annotations"]["readOnlyHint"], true);
    assert_eq!(tool["inputSchema"]["type"], "object");

    let resources = ok(&mut svc, JsonRpcRequest::new(2, "resources/list", None)).await;
    assert!(resources["resources"][0].get("icons").is_none());
    assert_eq!(resources["resources"][0]["title"], "A");

    let prompts = ok(&mut svc, JsonRpcRequest::new(3, "prompts/list", None)).await;
    assert!(prompts["prompts"][0].get("icons").is_none());
    assert_eq!(prompts["prompts"][0]["name"], "greet");

    // The control: a 2025-11-25 client gets both.
    let (mut svc, _) = connect("2025-11-25").await;
    let tools = ok(&mut svc, JsonRpcRequest::new(1, "tools/list", None)).await;
    let tool = &tools["tools"][0];
    assert_eq!(tool["icons"][0]["src"], "https://example.com/icon.png");
    assert_eq!(tool["execution"]["taskSupport"], "optional");
}

/// Everything the two revisions share must actually work, not merely fail
/// safely. `logging/setLevel` and `resources/subscribe` in particular are
/// legacy-only methods that both revisions define.
#[tokio::test]
async fn the_shared_method_surface_works() {
    let (mut svc, _) = connect("2025-06-18").await;

    let called = ok(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "name": "search", "arguments": {} })),
        ),
    )
    .await;
    assert_eq!(called["content"][0]["text"], "ok");
    assert!(
        called.get("resultType").is_none(),
        "the draft's envelope is not on this wire: {called}"
    );

    let read = ok(
        &mut svc,
        JsonRpcRequest::new(2, "resources/read", Some(json!({ "uri": "mem://a" }))),
    )
    .await;
    assert_eq!(read["contents"][0]["text"], "body");

    let prompt = ok(
        &mut svc,
        JsonRpcRequest::new(3, "prompts/get", Some(json!({ "name": "greet" }))),
    )
    .await;
    assert_eq!(prompt["messages"][0]["content"]["text"], "hi");

    ok(
        &mut svc,
        JsonRpcRequest::new(4, "logging/setLevel", Some(json!({ "level": "debug" }))),
    )
    .await;
    ok(
        &mut svc,
        JsonRpcRequest::new(5, "resources/subscribe", Some(json!({ "uri": "mem://a" }))),
    )
    .await;
    ok(&mut svc, JsonRpcRequest::new(6, "ping", Some(json!({})))).await;
}

/// A client asking for a revision this build doesn't serve still gets the
/// spec's fallback rather than a refusal — and the fallback is the latest
/// version that speaks `initialize` at all, never the draft (which negotiates
/// per request and has no handshake to answer with).
#[tokio::test]
async fn an_unservable_revision_falls_back_to_the_latest_stateful_one() {
    for requested in ["2024-11-05", "2025-03-26", "1999-01-01"] {
        let (_svc, init) = connect(requested).await;
        assert_eq!(
            init["protocolVersion"], "2025-11-25",
            "{requested} should fall back to the latest initialize-speaking version: {init}"
        );
    }
}

/// The two revisions are separate sessions on separate connections, so a
/// server serving both at once keeps them apart. This is the property the
/// per-session negotiated-version stamping exists for: get it wrong and
/// whichever client initialized last decides the wire shape for both.
#[tokio::test]
async fn two_revisions_are_served_concurrently_without_crosstalk() {
    let (mut old, _) = connect("2025-06-18").await;
    let (mut new, _) = connect("2025-11-25").await;

    for round in 1..=2 {
        let old_tools = ok(&mut old, JsonRpcRequest::new(round, "tools/list", None)).await;
        let new_tools = ok(&mut new, JsonRpcRequest::new(round, "tools/list", None)).await;
        assert!(
            old_tools["tools"][0].get("icons").is_none(),
            "round {round}: {old_tools}"
        );
        assert!(
            new_tools["tools"][0].get("icons").is_some(),
            "round {round}: {new_tools}"
        );
    }
}
