//! What survives wrapping a server in a layer.
//!
//! `MiddlewareStack`, `VisibilityLayer` and `CompositeHandler` each forward the
//! wrapped server's capabilities. Every method behind those capabilities has to
//! be forwarded too, or a client is told a feature exists and then gets
//! "method not found" for it.

use std::sync::{Arc, Mutex};

use turbomcp::prelude::*;
use turbomcp_core::handler::McpHandler;
use turbomcp_core::session::{McpSession, SessionFuture};
use turbomcp_server::{CompositeHandler, MiddlewareStack};

#[derive(Clone, Default)]
struct Full {
    subscriptions: Arc<Mutex<Vec<String>>>,
}

#[server(
    name = "full",
    version = "1.0.0",
    instructions = "Use noop.",
    page_size = 1
)]
impl Full {
    #[tool]
    async fn noop(&self) -> String {
        String::new()
    }

    #[tool(tags = ["admin"])]
    async fn other(&self) -> String {
        String::new()
    }

    /// Links one resource this server serves and one it does not.
    #[tool]
    async fn links(&self) -> ToolResult {
        ToolResult {
            content: vec![
                Content::ResourceLink(link("api/current")),
                Content::ResourceLink(link("https://example.com/doc")),
            ],
            ..Default::default()
        }
    }

    #[resource("api/current")]
    async fn current(&self, uri: String, _ctx: &RequestContext) -> McpResult<ResourceResult> {
        Ok(ResourceResult::text(&uri, "now"))
    }

    #[completion]
    async fn complete(&self, _params: serde_json::Value) -> McpResult<serde_json::Value> {
        Ok(serde_json::json!({ "completion": { "values": [] } }))
    }

    #[subscribe]
    async fn watch(&self, uri: String, ctx: &RequestContext) -> McpResult<()> {
        self.subscriptions.lock().unwrap().push(uri.clone());
        // With no session attached there is nobody to notify, which is not
        // a reason to refuse the subscription.
        let _ = ctx.notify_resource_updated(uri).await;
        Ok(())
    }

    #[unsubscribe]
    async fn unwatch(&self, _uri: String) -> McpResult<()> {
        Ok(())
    }
}

fn link(uri: &str) -> turbomcp_types::ResourceLink {
    serde_json::from_value(serde_json::json!({ "uri": uri, "name": uri })).expect("a resource link")
}

/// Records what a handler pushes to the client.
#[derive(Debug, Default)]
struct RecordingSession {
    notifications: Mutex<Vec<(String, serde_json::Value)>>,
}

impl McpSession for RecordingSession {
    fn call<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
    ) -> SessionFuture<'a, serde_json::Value> {
        Box::pin(async { Ok(serde_json::Value::Null) })
    }

    fn notify<'a>(&'a self, method: &'a str, params: serde_json::Value) -> SessionFuture<'a, ()> {
        Box::pin(async move {
            self.notifications
                .lock()
                .unwrap()
                .push((method.to_string(), params));
            Ok(())
        })
    }
}

async fn request_in<H: McpHandler>(
    handler: &H,
    ctx: RequestContext,
    method: &str,
    params: serde_json::Value,
) -> serde_json::Value {
    handler
        .handle_request(
            serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }),
            ctx,
        )
        .await
        .unwrap()
}

async fn request<H: McpHandler>(
    handler: &H,
    method: &str,
    params: serde_json::Value,
) -> serde_json::Value {
    request_in(handler, RequestContext::stdio(), method, params).await
}

/// Every advertised method answers, through the layer, as it does without it.
async fn assert_forwards_everything<H: McpHandler>(layer: &H) {
    let init = request(
        layer,
        "initialize",
        serde_json::json!({
            "protocolVersion": "2025-11-25",
            "clientInfo": { "name": "t", "version": "1" },
            "capabilities": {}
        }),
    )
    .await;
    assert_eq!(init["result"]["instructions"], "Use noop.", "{init}");

    for (method, params) in [
        ("logging/setLevel", serde_json::json!({ "level": "error" })),
        (
            "completion/complete",
            serde_json::json!({
                "ref": { "type": "ref/resource", "uri": "api/current" },
                "argument": { "name": "x", "value": "" }
            }),
        ),
        (
            "resources/subscribe",
            serde_json::json!({ "uri": "api/current" }),
        ),
        (
            "resources/unsubscribe",
            serde_json::json!({ "uri": "api/current" }),
        ),
    ] {
        let response = request(layer, method, params).await;
        assert!(
            response["error"].is_null(),
            "{method} is advertised, so it must not fail through the layer: {response}"
        );
    }

    // `page_size = 1` survives: the first page holds one tool and a cursor.
    let page = request(layer, "tools/list", serde_json::json!({})).await;
    assert_eq!(
        page["result"]["tools"].as_array().unwrap().len(),
        1,
        "{page}"
    );
    assert!(page["result"]["nextCursor"].is_string(), "{page}");
}

#[tokio::test]
async fn middleware_stack_forwards_every_advertised_method() {
    assert_forwards_everything(&MiddlewareStack::new(Full::default())).await;
}

#[tokio::test]
async fn visibility_layer_forwards_every_advertised_method() {
    assert_forwards_everything(&VisibilityLayer::new(Full::default())).await;
}

/// Every tool name `tools/list` offers `ctx`, across all pages.
async fn listed_tools<H: McpHandler>(handler: &H, ctx: RequestContext) -> Vec<String> {
    let mut names = Vec::new();
    let mut cursor = serde_json::Value::Null;
    loop {
        let page = request_in(
            handler,
            ctx.clone(),
            "tools/list",
            serde_json::json!({ "cursor": cursor }),
        )
        .await;
        names.extend(
            page["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["name"].as_str().unwrap().to_owned()),
        );
        cursor = page["result"]["nextCursor"].clone();
        if cursor.is_null() {
            return names;
        }
    }
}

/// Per-session overrides used to gate calls but not listings, so a client
/// told the catalogue changed re-listed and saw nothing new — while being
/// shown tools it could not call.
#[tokio::test]
async fn visibility_overrides_apply_to_what_a_session_is_shown() {
    let layer = VisibilityLayer::new(Full::default()).disable_tags(["admin"]);
    let ctx = || RequestContext::stdio().with_session_id("s-1");

    let before = listed_tools(&layer, ctx()).await;
    assert!(!before.contains(&"other".to_string()), "{before:?}");

    layer.enable_for_session("s-1", &["admin".to_string()]);
    let after = listed_tools(&layer, ctx()).await;
    assert!(
        after.contains(&"other".to_string()),
        "the session was granted `admin`, so `other` must be listed: {after:?}"
    );

    let call = request_in(
        &layer,
        ctx(),
        "tools/call",
        serde_json::json!({ "name": "other", "arguments": {} }),
    )
    .await;
    assert!(call["error"].is_null(), "{call}");
}

/// A mounted server's resource update used to carry its own unprefixed URI,
/// which matched nothing the client had subscribed to.
#[tokio::test]
async fn a_mounted_servers_resource_update_uses_the_composite_uri() {
    let composite = CompositeHandler::new("main", "1.0.0").mount(Full::default(), "weather");
    let session = Arc::new(RecordingSession::default());
    let ctx = RequestContext::stdio().with_session(session.clone() as Arc<dyn McpSession>);

    let response = request_in(
        &composite,
        ctx,
        "resources/subscribe",
        serde_json::json!({ "uri": "weather://api/current" }),
    )
    .await;
    assert!(response["error"].is_null(), "{response}");

    let sent = session.notifications.lock().unwrap().clone();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].0, "notifications/resources/updated");
    assert_eq!(sent[0].1["uri"], "weather://api/current");
}

/// Only URIs the mount serves are moved into its namespace. An external link
/// is one the client fetches itself.
#[tokio::test]
async fn a_composite_leaves_external_links_alone() {
    let composite = CompositeHandler::new("main", "1.0.0").mount(Full::default(), "weather");
    let response = request(
        &composite,
        "tools/call",
        serde_json::json!({ "name": "weather_links", "arguments": {} }),
    )
    .await;

    let content = &response["result"]["content"];
    assert_eq!(content[0]["uri"], "weather://api/current", "{response}");
    assert_eq!(content[1]["uri"], "https://example.com/doc", "{response}");
}

/// Every server can log through the request context, and the spec requires a
/// server that emits log messages to declare `logging` — and to accept
/// `logging/setLevel` once it has.
#[tokio::test]
async fn every_server_declares_logging_and_accepts_set_level() {
    #[derive(Clone)]
    struct Quiet;

    #[server(name = "quiet", version = "1.0.0")]
    impl Quiet {
        #[tool]
        async fn noop(&self) -> String {
            String::new()
        }
    }

    let init = request(
        &Quiet,
        "initialize",
        serde_json::json!({
            "protocolVersion": "2025-11-25",
            "clientInfo": { "name": "t", "version": "1" },
            "capabilities": {}
        }),
    )
    .await;
    assert!(
        init["result"]["capabilities"]["logging"].is_object(),
        "{init}"
    );

    let set = request(
        &Quiet,
        "logging/setLevel",
        serde_json::json!({ "level": "warning" }),
    )
    .await;
    assert!(set["error"].is_null(), "{set}");
}

/// The schema types a cursor as a string; anything else is invalid params,
/// not a silent restart from the first page.
#[tokio::test]
async fn a_non_string_cursor_is_invalid_params() {
    let response = request(
        &Full::default(),
        "tools/list",
        serde_json::json!({ "cursor": 5 }),
    )
    .await;
    assert_eq!(response["error"]["code"], -32602, "{response}");
}

/// The spec's resources error example carries the URI in `data`.
#[tokio::test]
async fn resource_not_found_names_the_uri() {
    let response = request(
        &Full::default(),
        "resources/read",
        serde_json::json!({ "uri": "api/nope" }),
    )
    .await;
    assert_eq!(response["error"]["code"], -32002, "{response}");
    assert_eq!(response["error"]["data"]["uri"], "api/nope", "{response}");
}
