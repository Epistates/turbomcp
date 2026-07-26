//! MRTR (SEP-2322) on the draft path: a handler's `ctx.client.elicit(…)`
//! becomes an `InputRequiredResult`, the client retries the SAME request with
//! `inputResponses` (+ echoed `requestState`), and the re-executed handler
//! completes — across `tools/call`, `prompts/get`, and `resources/read`.

use serde_json::{Value, json};
use tower::{Service, ServiceExt};
use turbomcp_core::{Implementation, JsonRpcMessage, JsonRpcRequest, McpError, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, GetPromptContext, LegacySessionAdapter, ListPromptsContext,
    ListResourcesContext, ListToolsContext, McpServerCore, MethodRouter, ReadResourceContext,
    VersionDispatcher, WithPrompts, WithResources, WithTools,
};

/// Every MRTR-capable handler elicits a `confirm` key before answering; the
/// tool additionally stores resume state and proves the retry sees it.
#[derive(Clone)]
struct Asker;

impl McpServerCore for Asker {
    fn server_info(&self) -> Implementation {
        Implementation::new("asker", "0.1.0")
    }
}

fn confirm_params() -> neutral::ElicitParams {
    neutral::ElicitParams::new(
        "Proceed?",
        json!({ "type": "object", "properties": { "ok": { "type": "boolean" } } }),
    )
}

impl WithTools for Asker {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![]))
    }

    async fn call_tool(
        &self,
        ctx: &CallToolContext,
        params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        match params.name.as_str() {
            // One elicitation + typed resume state.
            "guarded" => {
                // First execution: no state yet. Retry: state restored.
                let step: Option<u32> = ctx.client.load_state()?;
                if step.is_none() {
                    ctx.client.store_state(&42u32)?;
                }
                let outcome = ctx.client.elicit("confirm", confirm_params()).await?;
                // A retry carries the stored state; a same-pass completion
                // (response already present) ran with none.
                let state = step.map_or_else(|| "fresh".to_owned(), |s| s.to_string());
                Ok(neutral::CallToolResult::text(format!(
                    "action={:?} state={state}",
                    outcome.action
                )))
            }
            // Two inputs in one round trip (MR-4).
            "pair" => {
                let outcomes = ctx
                    .client
                    .elicit_all(vec![
                        ("first", confirm_params()),
                        ("second", confirm_params()),
                    ])
                    .await?;
                Ok(neutral::CallToolResult::text(format!(
                    "got {} answers",
                    outcomes.len()
                )))
            }
            // A handler that returns the abort sentinel by hand, having asked
            // for nothing — the misuse `finish_mrtr` has to catch.
            "leak" => Err(McpError::InputRequired),
            // Stores more resume state than a `requestState` may carry, so the
            // failure lands at signing time, after the handler is done.
            "hoard" => {
                ctx.client.store_state(&"x".repeat(64 * 1024))?;
                ctx.client.elicit("confirm", confirm_params()).await?;
                Ok(neutral::CallToolResult::text("unreachable"))
            }
            other => Err(McpError::tool_not_found(other)),
        }
    }
}

impl WithPrompts for Asker {
    async fn list_prompts(
        &self,
        _ctx: &ListPromptsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListPromptsResult> {
        Ok(neutral::ListPromptsResult::new(vec![]))
    }

    async fn get_prompt(
        &self,
        ctx: &GetPromptContext,
        _params: neutral::GetPromptParams,
    ) -> McpResult<neutral::GetPromptResult> {
        let outcome = ctx.client.elicit("confirm", confirm_params()).await?;
        Ok(neutral::GetPromptResult::new(vec![
            neutral::PromptMessage::user_text(format!("confirmed: {}", outcome.accepted())),
        ]))
    }
}

impl WithResources for Asker {
    async fn list_resources(
        &self,
        _ctx: &ListResourcesContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListResourcesResult> {
        Ok(neutral::ListResourcesResult::new(vec![]))
    }

    async fn read_resource(
        &self,
        ctx: &ReadResourceContext,
        params: neutral::ReadResourceParams,
    ) -> McpResult<neutral::ReadResourceResult> {
        let _outcome = ctx.client.elicit("confirm", confirm_params()).await?;
        Ok(neutral::ReadResourceResult::text(params.uri, "secret"))
    }
}

fn dispatcher() -> VersionDispatcher<Asker> {
    VersionDispatcher::new(
        Asker,
        MethodRouter::new()
            .with_tools()
            .with_prompts()
            .with_resources(),
    )
}

/// Draft `_meta` declaring the elicitation capability.
fn meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": { "name": "mrtr-client", "version": "1" },
        "io.modelcontextprotocol/clientCapabilities": { "elicitation": {} },
    })
}

/// A dispatcher past the `2025-11-25` handshake gate. Legacy requests carry a
/// session id, which only the adapter mints — the dispatcher rejects a legacy
/// capability request without one before it ever reaches dispatch.
async fn legacy_dispatcher() -> LegacySessionAdapter<VersionDispatcher<Asker>> {
    let mut svc = LegacySessionAdapter::new(dispatcher());
    let out = call(
        &mut svc,
        JsonRpcRequest::new(
            0,
            "initialize",
            Some(json!({
                "protocolVersion": "2025-11-25",
                "capabilities": { "elicitation": {} },
                "clientInfo": { "name": "legacy", "version": "1" },
            })),
        ),
    )
    .await;
    assert_eq!(out["protocolVersion"], "2025-11-25", "{out}");
    svc
}

fn accept() -> Value {
    json!({ "action": "accept", "content": { "ok": true } })
}

async fn call<S>(svc: &mut S, req: JsonRpcRequest) -> Value
where
    S: Service<JsonRpcMessage, Response = Option<JsonRpcMessage>>,
    S::Error: std::fmt::Debug,
{
    let out = svc
        .ready()
        .await
        .unwrap()
        .call(req.into())
        .await
        .unwrap()
        .expect("a response");
    let JsonRpcMessage::Response(r) = out else {
        panic!("expected response");
    };
    match (r.result, r.error) {
        (Some(result), None) => result,
        (_, Some(e)) => json!({ "error": { "code": e.code, "message": e.message } }),
        _ => panic!("empty response"),
    }
}

#[tokio::test]
async fn tool_elicit_round_trip_with_request_state() {
    let mut svc = dispatcher();

    // Round 1: the handler aborts with input_required + signed state.
    let first = call(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "name": "guarded", "arguments": {}, "_meta": meta() })),
        ),
    )
    .await;
    assert_eq!(first["resultType"], "input_required");
    let request = &first["inputRequests"]["confirm"];
    assert_eq!(request["method"], "elicitation/create");
    assert_eq!(request["params"]["mode"], "form");
    assert_eq!(request["params"]["message"], "Proceed?");
    let state = first["requestState"]
        .as_str()
        .expect("stored state must ride requestState")
        .to_owned();

    // Round 2: retry with the response + echoed state → the real result.
    let second = call(
        &mut svc,
        JsonRpcRequest::new(
            2,
            "tools/call",
            Some(json!({
                "name": "guarded", "arguments": {}, "_meta": meta(),
                "inputResponses": { "confirm": accept() },
                "requestState": state,
            })),
        ),
    )
    .await;
    assert_eq!(second["resultType"], "complete");
    assert_eq!(second["content"][0]["text"], "action=Accept state=42");
}

#[tokio::test]
async fn elicit_all_collects_one_round_trip() {
    let mut svc = dispatcher();
    let first = call(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "name": "pair", "arguments": {}, "_meta": meta() })),
        ),
    )
    .await;
    assert_eq!(first["resultType"], "input_required");
    let requests = first["inputRequests"].as_object().unwrap();
    assert_eq!(requests.len(), 2, "both inputs in ONE round trip");
    assert!(requests.contains_key("first") && requests.contains_key("second"));
    assert!(
        first.get("requestState").is_none(),
        "no state stored, none sent"
    );

    let second = call(
        &mut svc,
        JsonRpcRequest::new(
            2,
            "tools/call",
            Some(json!({
                "name": "pair", "arguments": {}, "_meta": meta(),
                "inputResponses": { "first": accept(), "second": accept() },
            })),
        ),
    )
    .await;
    assert_eq!(second["content"][0]["text"], "got 2 answers");
}

#[tokio::test]
async fn prompts_and_resources_support_mrtr_too() {
    let mut svc = dispatcher();

    let first = call(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "prompts/get",
            Some(json!({ "name": "p", "_meta": meta() })),
        ),
    )
    .await;
    assert_eq!(first["resultType"], "input_required");
    let second = call(
        &mut svc,
        JsonRpcRequest::new(
            2,
            "prompts/get",
            Some(json!({
                "name": "p", "_meta": meta(),
                "inputResponses": { "confirm": accept() },
            })),
        ),
    )
    .await;
    assert_eq!(second["messages"][0]["content"]["text"], "confirmed: true");

    let first = call(
        &mut svc,
        JsonRpcRequest::new(
            3,
            "resources/read",
            Some(json!({ "uri": "file://x", "_meta": meta() })),
        ),
    )
    .await;
    assert_eq!(first["resultType"], "input_required");
    let second = call(
        &mut svc,
        JsonRpcRequest::new(
            4,
            "resources/read",
            Some(json!({
                "uri": "file://x", "_meta": meta(),
                "inputResponses": { "confirm": accept() },
            })),
        ),
    )
    .await;
    assert_eq!(second["contents"][0]["text"], "secret");
}

#[tokio::test]
async fn tampered_request_state_is_rejected_before_the_handler_runs() {
    let mut svc = dispatcher();
    let out = call(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({
                "name": "guarded", "arguments": {}, "_meta": meta(),
                "inputResponses": { "confirm": accept() },
                "requestState": "v1.Zm9yZ2Vk.Zm9yZ2Vk",
            })),
        ),
    )
    .await;
    assert_eq!(out["error"]["code"], -32602);
    assert!(
        out["error"]["message"]
            .as_str()
            .unwrap()
            .contains("verification")
    );
}

#[tokio::test]
async fn undeclared_elicitation_capability_is_an_error_not_input_required() {
    let mut svc = dispatcher();
    // Same request, but the client declares NO capabilities.
    let bare_meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
    });
    let out = call(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "name": "guarded", "arguments": {}, "_meta": bare_meta })),
        ),
    )
    .await;
    // SEP-2322: the server MUST NOT send input requests the client didn't
    // declare — the call fails as a tool error (the handler's `?` propagated
    // a real error, not the abort sentinel).
    assert!(
        out.get("resultType").is_none() || out["resultType"] != "input_required",
        "must not answer input_required to a non-elicitation client: {out}"
    );
}

/// The spec requires an `InputRequiredResult` to carry at least one of
/// `inputRequests` / `requestState`. A handler that returns the sentinel
/// without asking for anything would otherwise produce a turn the client can
/// only answer by retrying identically — an infinite loop between a
/// well-behaved client and a buggy server. It is reported as a server error
/// instead.
#[tokio::test]
async fn a_bare_input_required_sentinel_is_a_server_error() {
    let mut svc = dispatcher();
    let out = call(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "name": "leak", "arguments": {}, "_meta": meta() })),
        ),
    )
    .await;
    assert_eq!(out["error"]["code"], -32603, "{out}");
    assert!(
        out["error"]["message"]
            .as_str()
            .unwrap()
            .contains("no input requests"),
        "{out}"
    );
}

/// Resume state is capped (PLAN MR-5) and the cap is enforced when the turn is
/// assembled — after the handler has already returned. That has to surface as
/// a clean error naming the limit, not a panic or a turn with the state
/// silently dropped (which would loop: the retry would re-store and re-fail).
#[tokio::test]
async fn an_oversized_resume_state_fails_the_turn_cleanly() {
    let mut svc = dispatcher();
    let out = call(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "name": "hoard", "arguments": {}, "_meta": meta() })),
        ),
    )
    .await;
    assert_eq!(out["error"]["code"], -32602, "{out}");
    assert!(
        out["error"]["message"].as_str().unwrap().contains("limit"),
        "{out}"
    );
}

/// MRTR is draft-only. On `2025-11-25` the same sentinel must come back as an
/// error, because a client on that version has no `input_required` resultType
/// — it would read the frame as a *successful* tool call and act on empty
/// content.
#[tokio::test]
async fn the_legacy_wire_never_answers_input_required() {
    let mut svc = legacy_dispatcher().await;
    let out = call(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "name": "leak", "arguments": {} })),
        ),
    )
    .await;
    assert!(
        out.get("resultType").is_none(),
        "the legacy wire has no MRTR turn: {out}"
    );
    assert_eq!(out["error"]["code"], -32603, "{out}");
}

/// The legacy path delivers client interaction as *inline* bidirectional
/// requests, which need a server→client channel. A client that initialized but
/// never opened one (no `GET` stream) must get a transport error naming the
/// fix — the alternative is the handler blocking for the full two-minute bidi
/// timeout on a channel that will never exist.
#[tokio::test]
async fn a_legacy_elicit_with_no_server_to_client_channel_fails_fast() {
    let mut svc = legacy_dispatcher().await;
    let out = call(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "name": "guarded", "arguments": {} })),
        ),
    )
    .await;
    let message = out["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("server→client channel"),
        "the message should name what is missing: {out}"
    );
}

#[tokio::test]
async fn decline_outcome_carries_no_content() {
    let mut svc = dispatcher();
    let out = call(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({
                "name": "guarded", "arguments": {}, "_meta": meta(),
                "inputResponses": {
                    "confirm": { "action": "decline", "content": { "smuggled": true } }
                },
            })),
        ),
    )
    .await;
    // The response was already present, so the handler completed on this
    // pass; the parsed outcome is Decline (and its smuggled content was
    // dropped — non-accept outcomes carry none).
    assert_eq!(out["resultType"], "complete");
    let text = out["content"][0]["text"].as_str().unwrap_or_default();
    assert!(text.contains("action=Decline"), "got: {out}");
}
