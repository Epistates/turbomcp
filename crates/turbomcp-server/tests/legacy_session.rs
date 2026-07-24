//! Legacy `2025-11-25` path integration: the `initialize` handshake, session
//! state injection, version negotiation, and the [`LegacySessionAdapter`]
//! per-connection flow — all at the `Service<JsonRpcMessage>` seam, no
//! transport required.

use serde_json::{Value, json};
use tower::{Service, ServiceExt};
use turbomcp_core::{
    Implementation, JsonRpcMessage, JsonRpcRequest, McpError, McpResult, ProtocolVersion, meta,
};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, LegacySessionAdapter, ListResourcesContext, ListToolsContext, McpServerCore,
    MethodRouter, ReadResourceContext, VersionDispatcher, WithResources, WithTools,
};
use turbomcp_service::ProtocolError;

/// A tools-only server whose `whoami` tool echoes the context's client info —
/// proving the session state actually reaches handlers.
#[derive(Clone)]
struct Echo;

impl McpServerCore for Echo {
    fn server_info(&self) -> Implementation {
        Implementation::new("echo-server", "1.0.0")
    }
    fn instructions(&self) -> Option<String> {
        Some("echoes".into())
    }
}

impl WithTools for Echo {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![neutral::Tool::new(
            "whoami",
            json!({"type": "object", "properties": {}}),
        )]))
    }

    async fn call_tool(
        &self,
        ctx: &CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        let client = ctx
            .base
            .client_info
            .as_ref()
            .map_or("<unknown>", |i| i.name.as_str());
        assert_eq!(ctx.base.protocol_version, ProtocolVersion::V2025_11_25);
        Ok(neutral::CallToolResult::text(format!("client: {client}")))
    }
}

/// Resources exist only to prove the version-split resource-not-found code
/// on this wire; every read misses.
impl WithResources for Echo {
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
        Err(McpError::resource_not_found(params.uri))
    }
}

fn adapter() -> LegacySessionAdapter<VersionDispatcher<Echo>> {
    LegacySessionAdapter::new(VersionDispatcher::new(
        Echo,
        MethodRouter::new().with_tools().with_resources(),
    ))
}

fn initialize_request(id: i64, version: &str) -> JsonRpcRequest {
    JsonRpcRequest::new(
        id,
        "initialize",
        Some(json!({
            "protocolVersion": version,
            "capabilities": { "roots": { "listChanged": true } },
            "clientInfo": { "name": "test-client", "version": "9.9" },
        })),
    )
}

async fn call<S>(svc: &mut S, msg: impl Into<JsonRpcMessage>) -> Option<JsonRpcMessage>
where
    S: Service<JsonRpcMessage, Response = Option<JsonRpcMessage>, Error = ProtocolError>,
{
    svc.ready()
        .await
        .expect("service ready")
        .call(msg.into())
        .await
        .expect("service call")
}

async fn call_result<S>(svc: &mut S, req: JsonRpcRequest) -> Value
where
    S: Service<JsonRpcMessage, Response = Option<JsonRpcMessage>, Error = ProtocolError>,
{
    match call(svc, req).await {
        Some(JsonRpcMessage::Response(r)) => {
            assert!(r.error.is_none(), "unexpected error: {:?}", r.error);
            r.result.expect("result")
        }
        other => panic!("expected response, got {other:?}"),
    }
}

#[tokio::test]
async fn full_legacy_handshake_and_tool_call_over_adapter() {
    let mut svc = adapter();

    // 1. initialize: exact-match negotiation echoes the requested version.
    let init = call_result(&mut svc, initialize_request(1, "2025-11-25")).await;
    assert_eq!(init["protocolVersion"], "2025-11-25");
    assert_eq!(init["serverInfo"]["name"], "echo-server");
    assert_eq!(init["instructions"], "echoes");
    assert_eq!(init["capabilities"]["tools"]["listChanged"], true);
    // Legacy initialize result carries no draft envelope.
    assert!(init.get("resultType").is_none());

    // 2. notifications/initialized passes through silently.
    let note = turbomcp_core::JsonRpcNotification::new("notifications/initialized", None);
    assert!(call(&mut svc, note).await.is_none());

    // 3. tools/list with NO version meta: the adapter stamps the session.
    let list = call_result(&mut svc, JsonRpcRequest::new(2, "tools/list", None)).await;
    assert_eq!(list["tools"][0]["name"], "whoami");
    assert!(list.get("resultType").is_none(), "legacy wire, not draft");
    assert!(list.get("cacheScope").is_none());

    // 4. tools/call sees the session's negotiated client info in its context.
    let called = call_result(
        &mut svc,
        JsonRpcRequest::new(3, "tools/call", Some(json!({ "name": "whoami" }))),
    )
    .await;
    assert_eq!(called["content"][0]["text"], "client: test-client");
    assert_eq!(called["isError"], false);
}

#[tokio::test]
async fn initialize_negotiates_down_to_latest_supported_legacy_version() {
    let mut svc = adapter();
    // An ancient client version we do not support → answer 2025-11-25, per
    // lifecycle spec ("respond with another protocol version it supports").
    let init = call_result(&mut svc, initialize_request(1, "2024-11-05")).await;
    assert_eq!(init["protocolVersion"], "2025-11-25");
}

#[tokio::test]
async fn malformed_initialize_does_not_enter_legacy_mode() {
    let mut svc = adapter();
    let bad = JsonRpcRequest::new(1, "initialize", Some(json!({ "nope": true })));
    let Some(JsonRpcMessage::Response(r)) = call(&mut svc, bad).await else {
        panic!("expected response")
    };
    assert_eq!(r.error.expect("invalid params").code, -32602);

    // The connection must still be uninitialized: a legacy-version request
    // (stamped manually, since the adapter won't) has no session.
    let meta = json!({ "io.modelcontextprotocol/protocolVersion": "2025-11-25" });
    let req = JsonRpcRequest::new(2, "tools/list", Some(json!({ "_meta": meta })));
    let Some(JsonRpcMessage::Response(r)) = call(&mut svc, req).await else {
        panic!("expected response")
    };
    assert_eq!(r.error.expect("not initialized").code, -32002);
}

#[tokio::test]
async fn unknown_session_id_is_a_protocol_error() {
    // Talk to the bare dispatcher the way a transport would: version + session
    // id injected via internal meta — but for a session that was never minted.
    let mut svc = VersionDispatcher::new(Echo, MethodRouter::new().with_tools());
    let mut msg: JsonRpcMessage = JsonRpcRequest::new(1, "tools/list", None).into();
    meta::set_request_meta(&mut msg, meta::keys::PROTOCOL_VERSION, json!("2025-11-25"));
    meta::set_request_meta(&mut msg, meta::internal::SESSION_ID, json!("never-minted"));

    let err = svc
        .ready()
        .await
        .unwrap()
        .call(msg)
        .await
        .expect_err("unknown session must surface as a protocol error");
    assert!(matches!(err, ProtocolError::UnknownSession(id) if id == "never-minted"));
}

#[tokio::test]
async fn forged_internal_session_meta_is_stripped_at_the_wire_boundary() {
    // Sanitization is the wire boundary's job (serve driver / HTTP endpoint),
    // not the adapter's — this simulates exactly what the driver does before
    // the adapter sees a frame. The forged key is gone, so the request is
    // "version present, no session" → -32002, NOT an unknown-session protocol
    // error (which would prove the forged id reached the dispatcher).
    let mut svc = adapter();
    let params = json!({
        "_meta": {
            "io.modelcontextprotocol/protocolVersion": "2025-11-25",
            meta::internal::SESSION_ID: "forged-id",
        }
    });
    let mut msg: JsonRpcMessage = JsonRpcRequest::new(1, "tools/list", Some(params)).into();
    meta::sanitize_inbound(&mut msg);
    let Some(JsonRpcMessage::Response(r)) = call(&mut svc, msg).await else {
        panic!("expected response")
    };
    assert_eq!(r.error.expect("not initialized").code, -32002);
}

#[tokio::test]
async fn modern_requests_keep_working_through_the_adapter_after_initialize() {
    let mut svc = adapter();
    let _ = call_result(&mut svc, initialize_request(1, "2025-11-25")).await;

    // A draft request states its version per-message; the adapter must leave
    // it alone and the dispatcher must answer on the draft wire.
    let params = json!({
        "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" }
    });
    let list = call_result(&mut svc, JsonRpcRequest::new(2, "tools/list", Some(params))).await;
    assert_eq!(list["resultType"], "complete", "draft wire envelope");
    assert_eq!(list["tools"][0]["name"], "whoami");
}

#[tokio::test]
async fn ping_answers_on_the_legacy_path() {
    let mut svc = adapter();
    let _ = call_result(&mut svc, initialize_request(1, "2025-11-25")).await;
    let pong = call_result(&mut svc, JsonRpcRequest::new(2, "ping", None)).await;
    assert_eq!(pong, json!({}));
}

/// Resource-not-found is **version-split**: `2025-11-25` prescribes `-32002`,
/// while the `2026-07-28` RC renumbered it to `-32602` (Invalid Params). A
/// legacy client keyed on its own spec's number must see that number — the
/// draft half is pinned in `dispatch_invariants.rs`.
#[tokio::test]
async fn resource_not_found_uses_the_legacy_code_on_this_wire() {
    let mut svc = adapter();
    let init = call_result(&mut svc, initialize_request(1, "2025-11-25")).await;
    assert_eq!(init["protocolVersion"], "2025-11-25");

    let read = JsonRpcRequest::new(2, "resources/read", Some(json!({ "uri": "mem://gone" })));
    let Some(JsonRpcMessage::Response(r)) = call(&mut svc, read).await else {
        panic!("expected a response")
    };
    let err = r.error.expect("resource-not-found error");
    assert_eq!(err.code, -32002, "{}", err.message);
}
