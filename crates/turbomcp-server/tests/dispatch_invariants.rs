//! Dispatcher spec invariants the rest of the suite only reaches implicitly:
//! modern-path version rejection (`-32004` with the supported list),
//! capability-derivation *enforcement* (unadvertised capability → `-32601`),
//! pagination-cursor plumbing, the malformed-params matrix (`-32602`), the
//! dual-version `server/discover` list, and the unknown-method catch-all.

use serde_json::{Value, json};
use tower::{Service, ServiceExt};
use turbomcp_core::{Implementation, JsonRpcMessage, JsonRpcRequest, McpError, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, CompleteContext, GetPromptContext, ListPromptsContext, ListResourcesContext,
    ListToolsContext, McpServerCore, MethodRouter, ReadResourceContext, VersionDispatcher,
    WithCompletions, WithPrompts, WithResources, WithTools,
};

/// A server advertising all four core capabilities; `list_tools` echoes the
/// received cursor into a tool name so tests can observe the plumbing.
#[derive(Clone)]
struct Kitchen;

impl McpServerCore for Kitchen {
    fn server_info(&self) -> Implementation {
        Implementation::new("kitchen", "1.0.0")
    }
}

impl WithTools for Kitchen {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        let name = match params.cursor {
            Some(c) => format!("page-{c}"),
            None => "page-first".into(),
        };
        let mut result = neutral::ListToolsResult::new(vec![neutral::Tool::new(
            name,
            json!({"type": "object"}),
        )]);
        result.next_cursor = Some("next-42".into());
        Ok(result)
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        Ok(neutral::CallToolResult::text("ok"))
    }
}

impl WithResources for Kitchen {
    async fn list_resources(
        &self,
        _ctx: &ListResourcesContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListResourcesResult> {
        Ok(neutral::ListResourcesResult::new(vec![]))
    }

    async fn read_resource(
        &self,
        _ctx: &ReadResourceContext,
        params: neutral::ReadResourceParams,
    ) -> McpResult<neutral::ReadResourceResult> {
        match params.uri.as_str() {
            "mem://a" => Ok(neutral::ReadResourceResult::text("mem://a", "hi")),
            other => Err(McpError::resource_not_found(other)),
        }
    }
}

impl WithPrompts for Kitchen {
    async fn list_prompts(
        &self,
        _ctx: &ListPromptsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListPromptsResult> {
        Ok(neutral::ListPromptsResult::new(vec![]))
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

impl WithCompletions for Kitchen {
    async fn complete(
        &self,
        _ctx: &CompleteContext,
        _params: neutral::CompleteParams,
    ) -> McpResult<neutral::CompleteResult> {
        Ok(neutral::CompleteResult::new(vec![]))
    }
}

fn kitchen() -> VersionDispatcher<Kitchen> {
    VersionDispatcher::new(
        Kitchen,
        MethodRouter::new()
            .with_tools()
            .with_resources()
            .with_prompts()
            .with_completions(),
    )
}

/// A server advertising ONLY tools — everything else must be `-32601`.
#[derive(Clone)]
struct ToolsOnly;

impl McpServerCore for ToolsOnly {
    fn server_info(&self) -> Implementation {
        Implementation::new("tools-only", "1.0.0")
    }
}

impl WithTools for ToolsOnly {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![]))
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        Ok(neutral::CallToolResult::text("ok"))
    }
}

fn draft_meta() -> Value {
    json!({ "io.modelcontextprotocol/protocolVersion": "2026-07-28" })
}

async fn call<S>(svc: &mut S, req: JsonRpcRequest) -> Value
where
    S: Service<JsonRpcMessage, Response = Option<JsonRpcMessage>>,
    S::Error: std::fmt::Debug,
{
    let JsonRpcMessage::Response(r) = svc
        .ready()
        .await
        .unwrap()
        .call(req.into())
        .await
        .unwrap()
        .expect("a response")
    else {
        panic!("expected a response")
    };
    json!({
        "result": r.result,
        "error": r.error.map(|e| json!({
            "code": e.code, "message": e.message, "data": e.data,
        })),
    })
}

/// PLAN §4.9: an unknown protocol version on a capability method answers
/// `-32004` and names the versions this build supports — in the message and,
/// as the RC requires, in `data: { supported, requested }` — so the client can
/// re-issue with one of them. A capability request with no version at all is
/// equally unsupported.
#[tokio::test]
async fn unknown_protocol_version_gets_32004_with_the_supported_list() {
    let mut svc = kitchen();
    let req = JsonRpcRequest::new(
        1,
        "tools/list",
        Some(json!({
            "_meta": { "io.modelcontextprotocol/protocolVersion": "1999-01-01" }
        })),
    );
    let out = call(&mut svc, req).await;
    assert_eq!(out["error"]["code"], -32004, "{out}");
    let msg = out["error"]["message"].as_str().unwrap();
    assert!(msg.contains("1999-01-01"), "names the requested: {msg}");
    assert!(
        msg.contains("2025-11-25") && msg.contains("2026-07-28"),
        "names the supported versions: {msg}"
    );
    // The RC pins the machine-readable payload, not just the prose.
    let data = &out["error"]["data"];
    assert_eq!(data["requested"], "1999-01-01", "{out}");
    assert_eq!(
        data["supported"],
        json!(["2025-11-25", "2026-07-28"]),
        "{out}"
    );

    let out = call(&mut svc, JsonRpcRequest::new(2, "tools/list", None)).await;
    assert_eq!(out["error"]["code"], -32004, "absent version: {out}");
}

/// The teeth of "capabilities are derived, not declared": a method whose
/// capability this server does not advertise answers `-32601`
/// (method-not-found), never `-32602` or a handler error. Both wire families
/// share this path (`dispatch_capability`).
#[tokio::test]
async fn unadvertised_capabilities_get_method_not_found() {
    let mut svc = VersionDispatcher::new(ToolsOnly, MethodRouter::new().with_tools());
    let cases = [
        (1, "prompts/list", json!({ "_meta": draft_meta() })),
        (
            2,
            "prompts/get",
            json!({ "name": "x", "_meta": draft_meta() }),
        ),
        (3, "resources/list", json!({ "_meta": draft_meta() })),
        (
            4,
            "resources/read",
            json!({ "uri": "mem://a", "_meta": draft_meta() }),
        ),
        (
            5,
            "resources/templates/list",
            json!({ "_meta": draft_meta() }),
        ),
        (
            6,
            "completion/complete",
            json!({
                "ref": { "type": "ref/prompt", "name": "x" },
                "argument": { "name": "a", "value": "" },
                "_meta": draft_meta(),
            }),
        ),
    ];
    for (id, method, params) in cases {
        let out = call(&mut svc, JsonRpcRequest::new(id, method, Some(params))).await;
        assert_eq!(
            out["error"]["code"], -32601,
            "{method} must be method-not-found: {out}"
        );
    }
    // The one advertised capability still answers.
    let out = call(
        &mut svc,
        JsonRpcRequest::new(7, "tools/list", Some(json!({ "_meta": draft_meta() }))),
    )
    .await;
    assert!(out["error"].is_null(), "{out}");
}

/// Cursors are opaque to the dispatcher: a request cursor must reach the
/// handler verbatim, a handler-returned `nextCursor` must reach the wire, and
/// a non-string cursor is leniently a first-page request (never an error).
#[tokio::test]
async fn list_cursor_reaches_the_handler_and_next_cursor_reaches_the_wire() {
    let mut svc = kitchen();

    let out = call(
        &mut svc,
        JsonRpcRequest::new(1, "tools/list", Some(json!({ "_meta": draft_meta() }))),
    )
    .await;
    assert_eq!(out["result"]["tools"][0]["name"], "page-first", "{out}");
    assert_eq!(out["result"]["nextCursor"], "next-42", "{out}");

    let out = call(
        &mut svc,
        JsonRpcRequest::new(
            2,
            "tools/list",
            Some(json!({ "cursor": "p2", "_meta": draft_meta() })),
        ),
    )
    .await;
    assert_eq!(out["result"]["tools"][0]["name"], "page-p2", "{out}");

    let out = call(
        &mut svc,
        JsonRpcRequest::new(
            3,
            "tools/list",
            Some(json!({ "cursor": 42, "_meta": draft_meta() })),
        ),
    )
    .await;
    assert!(out["error"].is_null(), "{out}");
    assert_eq!(out["result"]["tools"][0]["name"], "page-first", "{out}");
}

/// The `-32602` matrix for the param parsers: each distinct malformed shape is
/// invalid-params and the message names what is missing/wrong.
#[tokio::test]
async fn malformed_params_get_invalid_params() {
    let mut svc = kitchen();
    let cases = [
        (1, "resources/read", json!({ "_meta": draft_meta() }), "uri"),
        (2, "prompts/get", json!({ "_meta": draft_meta() }), "name"),
        (
            3,
            "completion/complete",
            json!({
                "ref": { "type": "ref/prompt" },
                "argument": { "name": "a", "value": "" },
                "_meta": draft_meta(),
            }),
            "name",
        ),
        (
            4,
            "completion/complete",
            json!({
                "ref": { "type": "ref/resource" },
                "argument": { "name": "a", "value": "" },
                "_meta": draft_meta(),
            }),
            "uri",
        ),
        (
            5,
            "completion/complete",
            json!({
                "ref": { "type": "ref/bogus" },
                "argument": { "name": "a", "value": "" },
                "_meta": draft_meta(),
            }),
            "ref",
        ),
    ];
    for (id, method, params, needle) in cases {
        let out = call(&mut svc, JsonRpcRequest::new(id, method, Some(params))).await;
        assert_eq!(out["error"]["code"], -32602, "{method}: {out}");
        assert!(
            out["error"]["message"].as_str().unwrap().contains(needle),
            "{method} message should mention '{needle}': {out}"
        );
    }
}

/// The dual-version headline: `server/discover` names BOTH supported versions.
#[tokio::test]
async fn discover_lists_both_supported_versions() {
    let mut svc = kitchen();
    let out = call(&mut svc, JsonRpcRequest::new(1, "server/discover", None)).await;
    let versions = out["result"]["supportedVersions"]
        .as_array()
        .unwrap_or_else(|| panic!("supportedVersions array: {out}"));
    assert!(
        versions.contains(&json!("2025-11-25")) && versions.contains(&json!("2026-07-28")),
        "{versions:?}"
    );
}

/// The catch-all arm: a method that exists on neither wire answers `-32601`,
/// with or without a version declared.
#[tokio::test]
async fn unknown_methods_are_method_not_found() {
    let mut svc = kitchen();
    let out = call(
        &mut svc,
        JsonRpcRequest::new(1, "does/not/exist", Some(json!({ "_meta": draft_meta() }))),
    )
    .await;
    assert_eq!(out["error"]["code"], -32601, "{out}");
    let out = call(&mut svc, JsonRpcRequest::new(2, "also/bogus", None)).await;
    assert_eq!(out["error"]["code"], -32601, "{out}");
}

/// JSON-RPC 2.0 §4: a frame *declaring* a version other than "2.0" is an
/// Invalid Request. A request answers `-32600`; a notification has no id to
/// answer with and is dropped. (A frame omitting the field entirely stays
/// tolerated — decode defaults it to "2.0" — pinned in `turbomcp-core`.)
#[tokio::test]
async fn wrong_jsonrpc_version_is_invalid_request() {
    let mut svc = kitchen();

    let mut req = JsonRpcRequest::new(1, "tools/list", Some(json!({ "_meta": draft_meta() })));
    req.jsonrpc = "1.0".into();
    let out = call(&mut svc, req).await;
    assert_eq!(out["error"]["code"], -32600, "{out}");
    assert!(
        out["error"]["message"].as_str().unwrap().contains("2.0"),
        "{out}"
    );

    let mut note = turbomcp_core::JsonRpcNotification::new("notifications/initialized", None);
    note.jsonrpc = "1.0".into();
    let reply = svc
        .ready()
        .await
        .unwrap()
        .call(JsonRpcMessage::Notification(note))
        .await
        .unwrap();
    assert!(reply.is_none(), "bad-version notification is dropped");
}

/// Resource-not-found is **version-split**: the `2026-07-28` RC renumbered it
/// to `-32602` (Invalid Params) to align with JSON-RPC, while `2025-11-25`
/// prescribes `-32002`. The draft half is pinned here; the legacy half rides
/// the session adapter in `legacy_session.rs`.
#[tokio::test]
async fn resource_not_found_is_invalid_params_on_the_draft() {
    let mut svc = kitchen();
    let out = call(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "resources/read",
            Some(json!({ "uri": "mem://gone", "_meta": draft_meta() })),
        ),
    )
    .await;
    assert_eq!(out["error"]["code"], -32602, "{out}");
}

/// The `2025-11-25`-only methods must not answer on the draft wire.
///
/// `resources/subscribe`, `resources/unsubscribe`, `logging/setLevel`, and the
/// four core `tasks/*` methods are session-scoped RPCs the draft deliberately
/// does not have — it replaced subscriptions with `subscriptions/listen`,
/// logging opt-in with a per-request `_meta` key, and core Tasks with an
/// extension. Each therefore routes on version *before* it routes on
/// capability, and the draft arm must be a plain `-32601`: answering one would
/// tell a draft client the server speaks a protocol it does not.
#[tokio::test]
async fn legacy_only_methods_are_method_not_found_on_the_draft() {
    for method in [
        "resources/subscribe",
        "resources/unsubscribe",
        "logging/setLevel",
        "tasks/list",
        "tasks/get",
        "tasks/cancel",
        "tasks/result",
    ] {
        let mut svc = kitchen();
        let out = call(
            &mut svc,
            JsonRpcRequest::new(
                1,
                method,
                Some(json!({ "uri": "mem://a", "level": "info", "_meta": draft_meta() })),
            ),
        )
        .await;
        assert_eq!(
            out["error"]["code"], -32601,
            "{method} must be method-not-found on the draft: {out}"
        );
        assert!(
            out["result"].is_null(),
            "{method} answered a result on the draft: {out}"
        );
    }
}

/// The same methods reach `-32004` — not `-32601` — for a version this build
/// does not serve at all. The distinction matters to a client: "I don't have
/// that method" is terminal, while "I don't speak that version" is something
/// it can act on by re-issuing, which is why the RC requires the supported
/// list to travel with the code.
#[tokio::test]
async fn legacy_only_methods_report_an_unsupported_version_as_32004() {
    for method in [
        "resources/subscribe",
        "resources/unsubscribe",
        "logging/setLevel",
        "tasks/get",
    ] {
        let mut svc = kitchen();
        let out = call(
            &mut svc,
            JsonRpcRequest::new(
                1,
                method,
                Some(json!({
                    "uri": "mem://a",
                    "level": "info",
                    "_meta": { "io.modelcontextprotocol/protocolVersion": "1999-01-01" }
                })),
            ),
        )
        .await;
        assert_eq!(out["error"]["code"], -32004, "{method}: {out}");
        assert_eq!(out["error"]["data"]["requested"], "1999-01-01", "{method}");
    }
}

/// An unsolicited client→server response is dropped, not answered.
///
/// Responses only ever arrive as replies to a server-initiated inline bidi
/// request (legacy elicitation/sampling/roots). One that matches nothing
/// pending is either a confused peer or a stale reply after a timeout; either
/// way, replying to a response would put a message on the wire that JSON-RPC
/// has no room for.
#[tokio::test]
async fn an_unsolicited_response_is_ignored() {
    let mut svc = kitchen();
    let reply = svc
        .ready()
        .await
        .unwrap()
        .call(JsonRpcMessage::Response(
            turbomcp_core::JsonRpcResponse::success(99, json!({ "action": "accept" })),
        ))
        .await
        .unwrap();
    assert!(reply.is_none(), "got {reply:?}");
}

/// `notifications/cancelled` is fire-and-forget: every malformed shape is
/// swallowed silently. Per spec a notification never draws a reply, so the
/// only failure this can have is a panic or a stray frame — which is exactly
/// what an unwrap on missing params or an unparseable id would produce.
#[tokio::test]
async fn malformed_cancellations_are_swallowed() {
    let cases = [
        // No params at all — no connection id to key the cancellation on.
        None,
        // A connection but no `requestId`.
        Some(json!({ "_meta": { "io.modelcontextprotocol/connectionId": "c1" } })),
        // `requestId` of the wrong type.
        Some(json!({
            "requestId": { "not": "an id" },
            "_meta": { "io.modelcontextprotocol/connectionId": "c1" }
        })),
        // Well-formed, but names a request that was never in flight.
        Some(json!({
            "requestId": 4242,
            "reason": "user pressed stop",
            "_meta": { "io.modelcontextprotocol/connectionId": "c1" }
        })),
    ];
    for (i, params) in cases.into_iter().enumerate() {
        let mut svc = kitchen();
        let reply = svc
            .ready()
            .await
            .unwrap()
            .call(JsonRpcMessage::Notification(
                turbomcp_core::JsonRpcNotification::new("notifications/cancelled", params),
            ))
            .await
            .unwrap();
        assert!(reply.is_none(), "case {i} drew a reply: {reply:?}");
    }
}
