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

    /// Takes the context, to prove the optional parameter is wired — and uses
    /// it to honour the subscription immediately, which is the whole point of
    /// accepting one.
    #[subscribe]
    async fn watch(&self, uri: String, ctx: &RequestContext) -> McpResult<()> {
        assert!(!ctx.request_id().is_empty());
        self.subscriptions.lock().unwrap().push(uri.clone());
        // A server that accepts a subscription owes the client updates.
        // Failing here would mean the transport has no session, which is not
        // an error for the subscription itself.
        let _ = ctx.notify_resource_updated(uri).await;
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
        // -32601: the completion spec spells this out as "Method not found:
        // -32601 (Capability not supported)", and an unsupported optional
        // method is exactly that.
        assert_eq!(
            response["error"]["code"], -32601,
            "{method} should report capability_not_supported as -32601, got {response}"
        );
    }
}

// ── The obligation a subscription creates ──────────────────────────────────

/// Accepting a subscription commits the server to sending updates. This pins
/// that `notify_resource_updated` puts the spec's shape on the wire, since
/// before 3.5.0 the only way to honour `#[subscribe]` was to hand-write the
/// method string.
#[tokio::test]
async fn subscribing_delivers_a_conformant_resource_updated_notification() {
    use turbomcp_core::session::{McpSession, SessionFuture};

    #[derive(Debug, Default)]
    struct Recorder {
        sent: Mutex<Vec<(String, serde_json::Value)>>,
    }

    impl McpSession for Recorder {
        fn call<'a>(
            &'a self,
            _m: &'a str,
            _p: serde_json::Value,
        ) -> SessionFuture<'a, serde_json::Value> {
            Box::pin(async { Ok(serde_json::Value::Null) })
        }
        fn notify<'a>(
            &'a self,
            method: &'a str,
            params: serde_json::Value,
        ) -> SessionFuture<'a, ()> {
            Box::pin(async move {
                self.sent.lock().unwrap().push((method.to_string(), params));
                Ok(())
            })
        }
    }

    let session = Arc::new(Recorder::default());
    let ctx = RequestContext::stdio().with_session(session.clone() as Arc<dyn McpSession>);

    let server = Full::default();
    let response = server
        .handle_request(
            serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "resources/subscribe",
                "params": { "uri": "mem://watched" }
            }),
            ctx,
        )
        .await
        .unwrap();
    assert!(response["error"].is_null(), "got {response}");

    let sent = session.sent.lock().unwrap().clone();
    assert_eq!(
        sent.len(),
        1,
        "the subscription should have produced one update"
    );
    assert_eq!(sent[0].0, "notifications/resources/updated");
    // `uri` is the only required param, and it must be the subscribed resource.
    assert_eq!(sent[0].1["uri"], "mem://watched");
}

// ── #[roots_changed] ───────────────────────────────────────────────────────

/// `McpHandler::on_roots_list_changed` shipped in 3.5.0, but the macro
/// generates a fixed method set — so without a marker it was unreachable for
/// macro-built servers, the same sealed-impl problem the other markers solve.
#[tokio::test]
async fn roots_changed_marker_reaches_the_handler() {
    #[derive(Clone, Default)]
    struct Watcher {
        invalidations: Arc<Mutex<usize>>,
    }

    #[server(name = "watcher", version = "1.0.0")]
    impl Watcher {
        #[tool]
        async fn noop(&self) -> String {
            String::new()
        }

        #[roots_changed]
        async fn roots_changed(&self, ctx: &RequestContext) -> McpResult<()> {
            assert!(!ctx.request_id().is_empty());
            *self.invalidations.lock().unwrap() += 1;
            Ok(())
        }
    }

    let server = Watcher::default();
    let response = server
        .handle_request(
            // A notification: no id, so no response is due.
            serde_json::json!({
                "jsonrpc": "2.0",
                "method": "notifications/roots/list_changed"
            }),
            RequestContext::stdio(),
        )
        .await
        .unwrap();

    // Notifications produce an internal ack envelope that transports suppress
    // (`should_send()` is false for it); what matters is that it carries
    // neither a result nor an error to send back.
    assert!(
        response.get("result").is_none() && response.get("error").is_none(),
        "a notification must not produce a response, got {response}"
    );
    assert_eq!(
        *server.invalidations.lock().unwrap(),
        1,
        "the roots-changed hook should have fired"
    );
}

// ── #[server(logging)] ─────────────────────────────────────────────────────

/// The `logging` capability means "this server emits `notifications/message`",
/// which is independent of implementing `logging/setLevel`. A server that logs
/// but does not let clients change the level previously had no way to declare
/// it, so it violated the rule that a server must declare what it uses.
#[tokio::test]
async fn logging_can_be_declared_without_a_set_level_handler() {
    #[derive(Clone)]
    struct Emitter;

    #[server(name = "emitter", version = "1.0.0", logging)]
    impl Emitter {
        #[tool]
        async fn noop(&self) -> String {
            String::new()
        }
    }

    let caps = capabilities_of(&Emitter).await;
    assert!(
        caps["logging"].is_object(),
        "#[server(logging)] must advertise the capability, got {caps}"
    );

    // Declaring it does not fabricate a setLevel handler.
    let response = request(
        &Emitter,
        "logging/setLevel",
        serde_json::json!({ "level": "debug" }),
    )
    .await;
    assert_eq!(response["error"]["code"], -32601);
}

/// And `#[set_level]` still implies it, so neither signal is load-bearing alone.
#[tokio::test]
async fn set_level_still_implies_the_logging_capability() {
    let caps = capabilities_of(&Full::default()).await;
    assert!(caps["logging"].is_object(), "got {caps}");
}

// ── logging/setLevel is validated, stored, and applied ─────────────────────

/// The eight RFC 5424 severities are a closed set in the schema, so anything
/// else is invalid params rather than something to hand a user's handler.
#[tokio::test]
async fn set_level_rejects_levels_outside_the_spec() {
    let server = Full::default();

    for bad in ["verbose", "trace", "DEBUG", "", "warn"] {
        let response = request(
            &server,
            "logging/setLevel",
            serde_json::json!({ "level": bad }),
        )
        .await;
        assert_eq!(
            response["error"]["code"], -32602,
            "level {bad:?} should be rejected, got {response}"
        );
    }
    assert!(
        server.levels.lock().unwrap().is_empty(),
        "an invalid level must never reach the handler"
    );

    // All eight legal levels are accepted.
    for good in [
        "debug",
        "info",
        "notice",
        "warning",
        "error",
        "critical",
        "alert",
        "emergency",
    ] {
        let response = request(
            &server,
            "logging/setLevel",
            serde_json::json!({ "level": good }),
        )
        .await;
        assert!(
            response["error"].is_null(),
            "level {good} rejected: {response}"
        );
    }
}

/// Accepting a level and then ignoring it is worse than refusing it: the client
/// believes its output is filtered when it is not.
#[tokio::test]
async fn the_set_level_actually_filters_emitted_messages() {
    use turbomcp_core::session::{McpSession, SessionFuture};
    use turbomcp_protocol::context::RichContextExt;
    use turbomcp_protocol::types::LogLevel;

    #[derive(Debug, Default)]
    struct Recorder {
        sent: Mutex<Vec<serde_json::Value>>,
    }
    impl McpSession for Recorder {
        fn call<'a>(
            &'a self,
            _m: &'a str,
            _p: serde_json::Value,
        ) -> SessionFuture<'a, serde_json::Value> {
            Box::pin(async { Ok(serde_json::Value::Null) })
        }
        fn notify<'a>(&'a self, _m: &'a str, params: serde_json::Value) -> SessionFuture<'a, ()> {
            Box::pin(async move {
                self.sent.lock().unwrap().push(params);
                Ok(())
            })
        }
    }

    let session = Arc::new(Recorder::default());
    let ctx = RequestContext::stdio()
        .with_session_id("filter-session")
        .with_session(session.clone() as Arc<dyn McpSession>);

    // Before any setLevel, everything is emitted.
    ctx.log(LogLevel::Debug, "before", None).await.unwrap();
    assert_eq!(session.sent.lock().unwrap().len(), 1);

    // Ask for `error` and above.
    let server = Full::default();
    server
        .handle_request(
            serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "logging/setLevel",
                "params": { "level": "error" }
            }),
            ctx.clone(),
        )
        .await
        .unwrap();

    ctx.log(LogLevel::Debug, "dropped", None).await.unwrap();
    ctx.log(LogLevel::Info, "dropped", None).await.unwrap();
    assert_eq!(
        session.sent.lock().unwrap().len(),
        1,
        "messages below the requested level must be suppressed"
    );

    ctx.log(LogLevel::Error, "kept", None).await.unwrap();
    ctx.log(LogLevel::Critical, "kept", None).await.unwrap();
    assert_eq!(
        session.sent.lock().unwrap().len(),
        3,
        "at or above the requested level must still be sent"
    );

    turbomcp_core::context::clear_min_log_level("filter-session");
}

// ── completion/complete params are validated ───────────────────────────────

/// `ref` and `argument` are both schema-required, and `argument` requires
/// string `name` and `value`. Validating centrally means a `#[completion]`
/// handler can index the shape it was promised instead of re-checking it.
#[tokio::test]
async fn completion_params_are_validated_before_the_handler() {
    let server = Full::default();

    let malformed = [
        serde_json::json!({}),
        serde_json::json!({ "argument": { "name": "topic", "value": "r" } }),
        serde_json::json!({ "ref": { "type": "ref/prompt" },
                            "argument": { "name": "topic", "value": "r" } }),
        serde_json::json!({ "ref": { "type": "ref/resource" },
                            "argument": { "name": "topic", "value": "r" } }),
        serde_json::json!({ "ref": { "type": "ref/nonsense", "name": "explain" },
                            "argument": { "name": "topic", "value": "r" } }),
        serde_json::json!({ "ref": { "type": "ref/prompt", "name": "explain" } }),
        serde_json::json!({ "ref": { "type": "ref/prompt", "name": "explain" },
                            "argument": { "name": "topic" } }),
        serde_json::json!({ "ref": { "type": "ref/prompt", "name": "explain" },
                            "argument": { "name": "topic", "value": 7 } }),
    ];

    for params in malformed {
        let response = request(&server, "completion/complete", params.clone()).await;
        assert_eq!(
            response["error"]["code"], -32602,
            "should be invalid params: {params}"
        );
    }

    // A ref/resource request is equally valid and must still reach the handler.
    let ok = request(
        &server,
        "completion/complete",
        serde_json::json!({
            "ref": { "type": "ref/resource", "uri": "mem://{id}" },
            "argument": { "name": "id", "value": "a" }
        }),
    )
    .await;
    assert!(ok["error"].is_null(), "got {ok}");
}
