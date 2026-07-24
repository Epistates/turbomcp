//! Phase 8a exit criterion: the [`Connection`] actor drives a real
//! `serve()` server over an in-memory duplex pipe — proving request/response
//! correlation, result-payload round-trip, and error routing through the whole
//! stack (`LineTransport` framing + `serve` driver + `VersionDispatcher`).
//!
//! Version negotiation is Phase 8b's job; here we stamp the modern version into
//! `_meta` by hand so the raw `request()` plumbing can be exercised on its own.

use serde_json::{Value, json};
use tokio::io::{BufReader, split};
use turbomcp_client::{ClientError, Connection};
use turbomcp_codec::DefaultCodec;
use turbomcp_core::{Implementation, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, ListToolsContext, McpServerCore, MethodRouter, VersionDispatcher, WithTools,
};
use turbomcp_transport_stdio::LineTransport;

#[derive(Clone)]
struct Calculator;

impl McpServerCore for Calculator {
    fn server_info(&self) -> Implementation {
        Implementation::new("calculator", "0.1.0")
    }
}

impl WithTools for Calculator {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![neutral::Tool::new(
            "add",
            json!({"type": "object"}),
        )]))
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        let a = params
            .arguments
            .get("a")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let b = params
            .arguments
            .get("b")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        Ok(neutral::CallToolResult::text((a + b).to_string()))
    }
}

/// Build params carrying the modern protocol version in `_meta` (what Phase 8b
/// will do automatically), optionally merging extra fields.
fn draft_params(extra: Value) -> Value {
    let mut obj = extra.as_object().cloned().unwrap_or_default();
    obj.insert(
        "_meta".into(),
        json!({ "io.modelcontextprotocol/protocolVersion": "2026-07-28" }),
    );
    Value::Object(obj)
}

/// Spawn the calculator server on one end of a duplex pipe; return a [`Connection`]
/// connected to the other end.
fn connected_client() -> Connection {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let (c_rd, c_wr) = split(client_io);
    let (s_rd, s_wr) = split(server_io);

    let dispatcher = VersionDispatcher::new(Calculator, MethodRouter::new().with_tools());
    let server_transport = LineTransport::new(BufReader::new(s_rd), s_wr, DefaultCodec::default());
    tokio::spawn(async move {
        let _ = turbomcp_service::serve(server_transport, dispatcher).await;
    });

    let client_transport = LineTransport::new(BufReader::new(c_rd), c_wr, DefaultCodec::default());
    Connection::new(client_transport)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ping_correlates_request_and_response() {
    let client = connected_client();
    // `ping` is answered before version classification — pure plumbing proof.
    let result = client.request("ping", None).await.expect("ping ok");
    assert!(result.is_object() || result.is_null());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tools_list_result_round_trips() {
    let client = connected_client();
    let result = client
        .request("tools/list", Some(draft_params(json!({}))))
        .await
        .expect("tools/list ok");
    // Assert on the raw wire shape — typed wire→neutral decoding is Phase 8b.
    let tools = result["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "add");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_requests_correlate_independently() {
    let client = connected_client();
    // Many in-flight at once: each reply must land on its own waiter.
    let mut handles = Vec::new();
    for _ in 0..16 {
        let c = client.clone();
        handles.push(tokio::spawn(async move {
            c.request("tools/list", Some(draft_params(json!({})))).await
        }));
    }
    for h in handles {
        let r = h.await.unwrap().expect("each request ok");
        assert_eq!(r["tools"].as_array().expect("tools array").len(), 1);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_error_surfaces_as_rpc_error() {
    let client = connected_client();
    // Missing protocolVersion on a modern request → -32004 (PLAN §4.9).
    let err = client
        .request("tools/list", Some(json!({})))
        .await
        .expect_err("should error without version");
    match err {
        ClientError::Rpc(e) => assert_eq!(e.code, -32004),
        other => panic!("expected Rpc(-32004), got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clones_share_one_connection() {
    let client = connected_client();
    let clone = client.clone();
    // Dropping one handle keeps the connection alive for the other.
    drop(client);
    clone
        .request("ping", None)
        .await
        .expect("ping via clone ok");
}
