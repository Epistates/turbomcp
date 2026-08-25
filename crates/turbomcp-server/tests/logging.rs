//! The `logging` capability (`notifications/message`): strictly opt-in on
//! both ends. Draft: per-request `_meta` `io.modelcontextprotocol/logLevel`
//! (an unrecognized level rejects the request, -32602). Legacy: per-session
//! `logging/setLevel`. Severity filters apply; without `with_logging()` the
//! capability is absent and `logging/setLevel` answers -32601.

use serde_json::{Value, json};
use tokio::sync::mpsc;
use turbomcp_core::{Implementation, JsonRpcMessage, LogLevel, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, ListToolsContext, McpServerCore, MethodRouter, VersionDispatcher, WithTools,
};
use turbomcp_service::{ServeConfig, Transport, serve_with};

struct MockTransport {
    inbound: mpsc::Receiver<JsonRpcMessage>,
    outbound: mpsc::UnboundedSender<JsonRpcMessage>,
}

impl Transport for MockTransport {
    type Error = std::io::Error;

    async fn send(&mut self, msg: JsonRpcMessage) -> Result<(), Self::Error> {
        self.outbound
            .send(msg)
            .map_err(|_| std::io::Error::other("outbound closed"))
    }

    async fn recv(&mut self) -> Result<Option<JsonRpcMessage>, Self::Error> {
        Ok(self.inbound.recv().await)
    }

    async fn close(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[derive(Clone)]
struct Chatty;

impl McpServerCore for Chatty {
    fn server_info(&self) -> Implementation {
        Implementation::new("chatty", "0.1.0")
    }
}

impl WithTools for Chatty {
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
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        ctx.log.debug(json!("noisy detail")).await;
        ctx.log
            .log_with(LogLevel::Error, Some("db"), json!({ "failed": true }))
            .await;
        Ok(neutral::CallToolResult::text("ok"))
    }
}

/// Run `frames` through a serve loop (logging enabled unless `plain`) and
/// collect everything the server writes.
async fn run(frames: Vec<Value>, logging: bool) -> Vec<Value> {
    let (in_tx, in_rx) = mpsc::channel(8);
    let (out_tx, mut out_rx) = mpsc::unbounded_channel();
    let mut router = MethodRouter::new().with_tools();
    if logging {
        router = router.with_logging();
    }
    let dispatcher = VersionDispatcher::new(Chatty, router);
    let service = turbomcp_server::LegacySessionAdapter::new(dispatcher);
    let transport = MockTransport {
        inbound: in_rx,
        outbound: out_tx,
    };
    let task = tokio::spawn(serve_with(transport, service, ServeConfig::default()));

    for frame in frames {
        in_tx
            .send(serde_json::from_value(frame).expect("valid frame"))
            .await
            .unwrap();
    }
    drop(in_tx);
    task.await.unwrap().expect("serve loop exits cleanly");

    let mut out = Vec::new();
    while let Ok(msg) = out_rx.try_recv() {
        out.push(serde_json::to_value(&msg).unwrap());
    }
    out
}

fn draft_call(id: u64, meta_extra: Value) -> Value {
    let mut meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
    });
    if let (Some(m), Some(extra)) = (meta.as_object_mut(), meta_extra.as_object()) {
        for (k, v) in extra {
            m.insert(k.clone(), v.clone());
        }
    }
    json!({
        "jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": { "name": "x", "arguments": {}, "_meta": meta }
    })
}

fn legacy_init(id: u64) -> Value {
    json!({
        "jsonrpc": "2.0", "id": id, "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "legacy", "version": "1" },
        }
    })
}

// ---- draft: per-request opt-in --------------------------------------------------

#[tokio::test]
async fn draft_opt_in_filters_by_severity() {
    let frames = run(
        vec![draft_call(
            1,
            json!({ "io.modelcontextprotocol/logLevel": "info" }),
        )],
        true,
    )
    .await;
    // debug filtered; error delivered before the response.
    assert_eq!(frames.len(), 2, "got: {frames:#?}");
    assert_eq!(frames[0]["method"], "notifications/message");
    assert_eq!(frames[0]["params"]["level"], "error");
    assert_eq!(frames[0]["params"]["logger"], "db");
    assert_eq!(frames[0]["params"]["data"]["failed"], true);
    assert_eq!(frames[1]["id"], 1);
}

#[tokio::test]
async fn draft_no_opt_in_means_no_messages() {
    let frames = run(vec![draft_call(1, json!({}))], true).await;
    assert_eq!(frames.len(), 1, "just the response");
}

#[tokio::test]
async fn draft_debug_opt_in_delivers_everything() {
    let frames = run(
        vec![draft_call(
            1,
            json!({ "io.modelcontextprotocol/logLevel": "debug" }),
        )],
        true,
    )
    .await;
    assert_eq!(frames.len(), 3, "debug + error + response");
    assert_eq!(frames[0]["params"]["level"], "debug");
}

#[tokio::test]
async fn draft_invalid_level_rejects_the_request() {
    let frames = run(
        vec![draft_call(
            1,
            json!({ "io.modelcontextprotocol/logLevel": "verbose" }),
        )],
        true,
    )
    .await;
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0]["error"]["code"], -32602);
}

#[tokio::test]
async fn capability_absent_drops_messages_even_with_opt_in() {
    let frames = run(
        vec![draft_call(
            1,
            json!({ "io.modelcontextprotocol/logLevel": "debug" }),
        )],
        false,
    )
    .await;
    assert_eq!(frames.len(), 1, "no capability → no messages");
}

// ---- legacy: per-session setLevel ------------------------------------------------

fn legacy_call(id: u64) -> Value {
    json!({
        "jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": { "name": "x", "arguments": {} }
    })
}

fn set_level(id: u64, level: &str) -> Value {
    json!({
        "jsonrpc": "2.0", "id": id, "method": "logging/setLevel",
        "params": { "level": level }
    })
}

#[tokio::test]
async fn legacy_set_level_enables_filtered_delivery() {
    let frames = run(
        vec![
            legacy_init(1),
            legacy_call(2),       // before setLevel: suppressed
            set_level(3, "info"), // opt in
            legacy_call(4),       // error delivered, debug filtered
        ],
        true,
    )
    .await;
    // init response advertises logging; pre-opt-in call emits nothing.
    assert!(
        frames[0]["result"]["capabilities"]["logging"].is_object(),
        "logging capability advertised: {frames:#?}"
    );
    assert_eq!(frames[1]["id"], 2);
    assert_eq!(frames[2]["id"], 3);
    assert_eq!(frames[2]["result"], json!({}));
    assert_eq!(frames[3]["method"], "notifications/message");
    assert_eq!(frames[3]["params"]["level"], "error");
    assert_eq!(frames[4]["id"], 4);
    assert_eq!(frames.len(), 5);
}

#[tokio::test]
async fn legacy_set_level_rejections() {
    // Invalid level → -32602; with logging disabled → -32601; on the draft →
    // -32601 (the RPC was replaced by the per-request key).
    let frames = run(vec![legacy_init(1), set_level(2, "chatty")], true).await;
    assert_eq!(frames[1]["error"]["code"], -32602);
    assert!(frames[0]["result"]["capabilities"].get("logging").is_some());

    let frames = run(vec![legacy_init(1), set_level(2, "info")], false).await;
    assert_eq!(frames[1]["error"]["code"], -32601);
    assert!(
        frames[0]["result"]["capabilities"].get("logging").is_none(),
        "no capability without with_logging()"
    );

    let frames = run(
        vec![json!({
            "jsonrpc": "2.0", "id": 1, "method": "logging/setLevel",
            "params": {
                "level": "info",
                "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" },
            }
        })],
        true,
    )
    .await;
    assert_eq!(frames[0]["error"]["code"], -32601);
}
