//! Wire names are decoupled from Rust identifiers.
//!
//! A tool's name is a public contract. Welding it to the Rust method name means
//! an internal rename silently breaks every client, and rules out names that
//! aren't valid Rust identifiers (`search.web`, `list-files`) — which real MCP
//! servers do use. `name = "…"` separates the two; the method name remains the
//! default.
//!
//! This file also pins that two `#[server]` impls can coexist in one module:
//! the generated per-tool argument structs are qualified by the server type, so
//! a shared tool name no longer collides.

use serde_json::{Map, Value, json};
use tower::{Service, ServiceExt};
use turbomcp::prelude::*;
use turbomcp::{JsonRpcMessage, JsonRpcRequest, McpServerCore};

#[derive(Clone)]
struct Renamed;

#[server(name = "renamed", version = "1.0.0")]
impl Renamed {
    /// Exposed under a name that is not a valid Rust identifier.
    #[tool(name = "search.web", description = "Search the web")]
    async fn search_web(&self, q: String) -> String {
        format!("searched {q}")
    }

    /// Default: the method name.
    #[tool]
    async fn plain(&self) -> String {
        "plain".into()
    }

    #[prompt(name = "summarize-text", description = "Summarize")]
    async fn summarize(&self, text: String) -> McpResult<String> {
        Ok(format!("summary: {text}"))
    }
}

/// A second server in the same module, sharing a tool name with the first.
#[derive(Clone)]
struct Other;

#[server(name = "other", version = "1.0.0")]
impl Other {
    #[tool]
    async fn plain(&self) -> String {
        "other".into()
    }
}

/// Dispatch one request against an already-built dispatcher.
///
/// Takes the *built* service rather than the server value: `#[server]` emits an
/// inherent `into_server()` that pre-registers the discovered capabilities, and
/// inherent methods only shadow the blanket `IntoServerBuilder::into_server`
/// when the receiver's type is concrete. A generic helper would silently get the
/// capability-less blanket one.
async fn call<S>(mut svc: turbomcp::VersionDispatcher<S>, req: JsonRpcRequest) -> Value
where
    S: McpServerCore + Clone,
{
    let resp = svc
        .ready()
        .await
        .expect("ready")
        .call(JsonRpcMessage::Request(req))
        .await
        .expect("dispatch");
    match resp {
        Some(JsonRpcMessage::Response(r)) => serde_json::to_value(r).expect("serialize"),
        other => panic!("expected a response, got {other:?}"),
    }
}

fn draft(method: &str, mut params: Map<String, Value>) -> JsonRpcRequest {
    params.insert(
        "_meta".into(),
        json!({ "io.modelcontextprotocol/protocolVersion": "2026-07-28" }),
    );
    JsonRpcRequest::new(1, method, Some(Value::Object(params)))
}

#[tokio::test]
async fn tools_list_reports_the_wire_name() {
    let body = call(
        Renamed.into_server().build(),
        draft("tools/list", Map::new()),
    )
    .await;
    let names: Vec<_> = body["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert_eq!(names, ["search.web", "plain"]);
}

#[tokio::test]
async fn a_renamed_tool_is_called_by_its_wire_name() {
    let mut params = Map::new();
    params.insert("name".into(), json!("search.web"));
    params.insert("arguments".into(), json!({ "q": "rust" }));
    let body = call(Renamed.into_server().build(), draft("tools/call", params)).await;
    assert_eq!(
        body["result"]["content"][0]["text"], "searched rust",
        "got {body}"
    );

    // The Rust method name is not a wire name once `name = "…"` is set.
    let mut params = Map::new();
    params.insert("name".into(), json!("search_web"));
    params.insert("arguments".into(), json!({ "q": "rust" }));
    let body = call(Renamed.into_server().build(), draft("tools/call", params)).await;
    assert_eq!(
        body["result"]["isError"], true,
        "the Rust name must not also resolve: {body}"
    );
}

#[tokio::test]
async fn an_unrenamed_tool_still_uses_its_method_name() {
    let mut params = Map::new();
    params.insert("name".into(), json!("plain"));
    params.insert("arguments".into(), json!({}));
    let body = call(Renamed.into_server().build(), draft("tools/call", params)).await;
    assert_eq!(body["result"]["content"][0]["text"], "plain");
}

#[tokio::test]
async fn prompts_honor_the_wire_name_too() {
    let body = call(
        Renamed.into_server().build(),
        draft("prompts/list", Map::new()),
    )
    .await;
    assert_eq!(body["result"]["prompts"][0]["name"], "summarize-text");

    let mut params = Map::new();
    params.insert("name".into(), json!("summarize-text"));
    params.insert("arguments".into(), json!({ "text": "hello" }));
    let body = call(Renamed.into_server().build(), draft("prompts/get", params)).await;
    assert_eq!(
        body["result"]["messages"][0]["content"]["text"], "summary: hello",
        "got {body}"
    );
}

/// Two servers in one module, both with a `plain` tool: the generated argument
/// structs must not collide, and each must dispatch to its own handler.
#[tokio::test]
async fn two_servers_in_one_module_may_share_a_tool_name() {
    let mut params = Map::new();
    params.insert("name".into(), json!("plain"));
    params.insert("arguments".into(), json!({}));
    let a = call(
        Renamed.into_server().build(),
        draft("tools/call", params.clone()),
    )
    .await;
    let b = call(Other.into_server().build(), draft("tools/call", params)).await;
    assert_eq!(a["result"]["content"][0]["text"], "plain");
    assert_eq!(b["result"]["content"][0]["text"], "other");
}
