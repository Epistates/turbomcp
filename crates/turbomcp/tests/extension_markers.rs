//! `#[completion]`, `#[subscribe]`, `#[unsubscribe]`, `#[set_level]`.
//!
//! Before 3.5.0 `#[server]` generated a sealed `impl McpHandler`, so five of
//! the trait's extension points had no override hook. Completions, resource
//! subscriptions, and logging were therefore unreachable for every macro-built
//! server — which is every server. These markers open them, and each one also
//! flips the matching capability so what `initialize` advertises stays equal to
//! what the server can actually serve.

use std::sync::{Arc, Mutex};

use turbomcp::prelude::*;

#[derive(Clone, Default)]
struct Full {
    levels: Arc<Mutex<Vec<String>>>,
    subscriptions: Arc<Mutex<Vec<String>>>,
}

#[server(name = "full", version = "1.0.0")]
impl Full {
    #[tool]
    async fn noop(&self) -> String {
        String::new()
    }

    #[prompt]
    async fn explain(&self, topic: String, _ctx: &RequestContext) -> McpResult<PromptResult> {
        Ok(PromptResult::user(format!("about {topic}")))
    }

    /// Completes a prompt argument from a fixed vocabulary.
    #[completion]
    async fn complete(&self, params: serde_json::Value) -> McpResult<serde_json::Value> {
        let prefix = params["argument"]["value"].as_str().unwrap_or("");
        let values: Vec<&str> = ["rust", "ruby", "racket"]
            .into_iter()
            .filter(|lang| lang.starts_with(prefix))
            .collect();
        Ok(serde_json::json!({ "completion": { "values": values } }))
    }

    /// Takes the context, to prove the optional parameter is wired.
    #[subscribe]
    async fn watch(&self, uri: String, ctx: &RequestContext) -> McpResult<()> {
        assert!(!ctx.request_id().is_empty());
        self.subscriptions.lock().unwrap().push(uri);
        Ok(())
    }

    #[unsubscribe]
    async fn unwatch(&self, uri: String) -> McpResult<()> {
        self.subscriptions.lock().unwrap().retain(|u| *u != uri);
        Ok(())
    }

    #[set_level]
    async fn set_level(&self, level: String) -> McpResult<()> {
        self.levels.lock().unwrap().push(level);
        Ok(())
    }
}

/// Declares none of the markers, to pin the negative case.
#[derive(Clone)]
struct Bare;

#[server(name = "bare", version = "1.0.0")]
impl Bare {
    #[tool]
    async fn noop(&self) -> String {
        String::new()
    }
}

async fn request<H: turbomcp_core::handler::McpHandler>(
    handler: &H,
    method: &str,
    params: serde_json::Value,
) -> serde_json::Value {
    handler
        .handle_request(
            serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": method, "params": params
            }),
            RequestContext::stdio(),
        )
        .await
        .unwrap()
}

async fn capabilities_of<H: turbomcp_core::handler::McpHandler>(handler: &H) -> serde_json::Value {
    let response = request(
        handler,
        "initialize",
        serde_json::json!({
            "protocolVersion": "2025-11-25",
            "clientInfo": { "name": "t", "version": "1" },
            "capabilities": {}
        }),
    )
    .await;
    response["result"]["capabilities"].clone()
}

// ── The handlers are reachable ─────────────────────────────────────────────

#[tokio::test]
async fn completion_marker_answers_completion_complete() {
    let response = request(
        &Full::default(),
        "completion/complete",
        serde_json::json!({
            "ref": { "type": "ref/prompt", "name": "explain" },
            "argument": { "name": "topic", "value": "r" }
        }),
    )
    .await;

    assert!(response["error"].is_null(), "got {response}");
    let values = &response["result"]["completion"]["values"];
    assert_eq!(values.as_array().unwrap().len(), 3);

    let narrowed = request(
        &Full::default(),
        "completion/complete",
        serde_json::json!({
            "ref": { "type": "ref/prompt", "name": "explain" },
            "argument": { "name": "topic", "value": "ru" }
        }),
    )
    .await;
    assert_eq!(
        narrowed["result"]["completion"]["values"],
        serde_json::json!(["rust", "ruby"])
    );
}

#[tokio::test]
async fn subscribe_and_unsubscribe_markers_reach_the_handler() {
    let server = Full::default();

    let subscribed = request(
        &server,
        "resources/subscribe",
        serde_json::json!({ "uri": "mem://doc" }),
    )
    .await;
    assert!(subscribed["error"].is_null(), "got {subscribed}");
    assert_eq!(
        server.subscriptions.lock().unwrap().as_slice(),
        ["mem://doc"]
    );

    let unsubscribed = request(
        &server,
        "resources/unsubscribe",
        serde_json::json!({ "uri": "mem://doc" }),
    )
    .await;
    assert!(unsubscribed["error"].is_null(), "got {unsubscribed}");
    assert!(server.subscriptions.lock().unwrap().is_empty());
}

#[tokio::test]
async fn set_level_marker_reaches_the_handler() {
    let server = Full::default();

    let response = request(
        &server,
        "logging/setLevel",
        serde_json::json!({ "level": "warning" }),
    )
    .await;

    assert!(response["error"].is_null(), "got {response}");
    assert_eq!(server.levels.lock().unwrap().as_slice(), ["warning"]);
}

// ── Capabilities track the markers ─────────────────────────────────────────

#[tokio::test]
async fn declaring_markers_advertises_the_capabilities() {
    let caps = capabilities_of(&Full::default()).await;

    assert!(caps["completions"].is_object(), "got {caps}");
    assert!(caps["logging"].is_object(), "got {caps}");
    // No `#[resource]` on this server, yet `#[subscribe]` alone must still
    // surface the resources capability — dynamic resources need no listing.
    assert_eq!(caps["resources"]["subscribe"], true);
    assert_eq!(caps["tools"]["listChanged"], true);
    assert_eq!(caps["prompts"]["listChanged"], true);
}

#[tokio::test]
async fn omitting_markers_advertises_nothing_extra() {
    let caps = capabilities_of(&Bare).await;

    // The central guarantee: never claim a capability we would then answer
    // with `capability_not_supported`.
    assert!(caps["completions"].is_null(), "got {caps}");
    assert!(caps["logging"].is_null(), "got {caps}");
    assert!(caps["resources"].is_null(), "got {caps}");
    assert_eq!(caps["tools"]["listChanged"], true);
}

#[tokio::test]
async fn undeclared_extension_points_still_report_unsupported() {
    for (method, params) in [
        ("completion/complete", serde_json::json!({})),
        ("resources/subscribe", serde_json::json!({ "uri": "x://y" })),
        (
            "resources/unsubscribe",
            serde_json::json!({ "uri": "x://y" }),
        ),
        ("logging/setLevel", serde_json::json!({ "level": "debug" })),
    ] {
        let response = request(&Bare, method, params).await;
        assert_eq!(
            response["error"]["code"], -32006,
            "{method} should report capability_not_supported, got {response}"
        );
    }
}
