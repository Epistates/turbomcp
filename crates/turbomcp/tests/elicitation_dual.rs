//! Phase 6 exit criterion: elicitation runs end-to-end on **both transports
//! and both protocol versions** from one macro-generated `#[server]` — MRTR
//! (`InputRequiredResult` + retry) on the draft, inline bidirectional
//! `elicitation/create` on `2025-11-25`.

#![cfg(feature = "http")]

use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tower::ServiceExt; // oneshot
use turbomcp::http::{HttpConfig, router};
use turbomcp::prelude::*;
use turbomcp::{LegacySessionAdapter, SerdeJsonCodec, serve};
use turbomcp_service::io::LineTransport;

#[derive(Clone)]
struct Deleter;

#[server(name = "deleter", version = "1.0.0")]
impl Deleter {
    /// Delete a file, but only after the user confirms.
    #[tool(description = "Delete a path after user confirmation")]
    async fn delete(&self, ctx: &CallToolContext, path: String) -> McpResult<String> {
        let outcome = ctx
            .client
            .elicit(
                "confirm",
                neutral::ElicitParams::new(
                    format!("Delete {path}?"),
                    json!({ "type": "object", "properties": { "ok": { "type": "boolean" } } }),
                ),
            )
            .await?;
        Ok(if outcome.accepted() {
            format!("deleted {path}")
        } else {
            "kept".to_owned()
        })
    }
}

/// The mirrored request-metadata headers the draft transport requires on a
/// `tools/call` POST.
const DRAFT_HEADERS: &[(&str, &str)] = &[
    ("mcp-protocol-version", "2026-07-28"),
    ("mcp-method", "tools/call"),
    ("mcp-name", "delete"),
];

fn draft_meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": { "elicitation": {} },
    })
}

fn draft_call(id: u64, extra: Value) -> Value {
    let mut params = json!({
        "name": "delete",
        "arguments": { "path": "/tmp/x" },
        "_meta": draft_meta(),
    });
    if let (Some(obj), Some(more)) = (params.as_object_mut(), extra.as_object()) {
        for (k, v) in more {
            obj.insert(k.clone(), v.clone());
        }
    }
    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": params })
}

fn accept() -> Value {
    json!({ "action": "accept", "content": { "ok": true } })
}

// ---- stdio framing: both versions on one connection -----------------------------

#[tokio::test]
async fn elicitation_works_on_both_versions_over_stdio_framing() {
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (server_rx, server_tx) = tokio::io::split(server);
    let transport = LineTransport::new(BufReader::new(server_rx), server_tx, SerdeJsonCodec);
    let service = LegacySessionAdapter::new(Deleter.into_server().build());
    let task = tokio::spawn(serve(transport, service));

    let (client_rx, mut client_tx) = tokio::io::split(client);
    let mut client_rx = BufReader::new(client_rx);

    let mut send = async |frame: Value| {
        client_tx
            .write_all(format!("{frame}\n").as_bytes())
            .await
            .unwrap();
        client_tx.flush().await.unwrap();
    };
    macro_rules! recv {
        () => {{
            let mut line = String::new();
            tokio::time::timeout(Duration::from_secs(5), async {
                client_rx.read_line(&mut line).await.unwrap();
            })
            .await
            .expect("a frame should arrive");
            serde_json::from_str::<Value>(line.trim()).unwrap()
        }};
    }

    // Draft MRTR: call → input_required → retry-with-response → result.
    send(draft_call(1, json!({}))).await;
    let first = recv!();
    assert_eq!(first["result"]["resultType"], "input_required");
    assert_eq!(
        first["result"]["inputRequests"]["confirm"]["method"],
        "elicitation/create"
    );
    send(draft_call(
        2,
        json!({ "inputResponses": { "confirm": accept() } }),
    ))
    .await;
    let second = recv!();
    assert_eq!(second["result"]["content"][0]["text"], "deleted /tmp/x");

    // Legacy inline bidi on the SAME connection: handshake, then the call
    // blocks while the server's own elicitation request comes down the pipe.
    send(json!({
        "jsonrpc": "2.0", "id": 3, "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": { "elicitation": {} },
            "clientInfo": { "name": "legacy", "version": "1" },
        }
    }))
    .await;
    let init = recv!();
    assert_eq!(init["result"]["protocolVersion"], "2025-11-25");
    send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await;

    send(json!({
        "jsonrpc": "2.0", "id": 4, "method": "tools/call",
        "params": { "name": "delete", "arguments": { "path": "/tmp/y" } }
    }))
    .await;
    let elicit = recv!();
    assert_eq!(elicit["method"], "elicitation/create");
    assert_eq!(elicit["params"]["message"], "Delete /tmp/y?");
    let srv_id = elicit["id"].clone();

    send(json!({ "jsonrpc": "2.0", "id": srv_id, "result": accept() })).await;
    let done = recv!();
    assert_eq!(done["id"], 4);
    assert_eq!(done["result"]["content"][0]["text"], "deleted /tmp/y");

    client_tx.shutdown().await.unwrap();
    task.await.unwrap().expect("serve loop ends cleanly on EOF");
}

// ---- HTTP: both versions --------------------------------------------------------

fn post(body: Value, headers: &[(&str, &str)]) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .header("accept", "application/json, text/event-stream")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    req.body(Body::from(body.to_string())).unwrap()
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

/// One complete SSE event off a streaming body.
async fn next_sse_event(body: &mut Body, buffer: &mut String) -> Value {
    loop {
        if let Some(end) = buffer.find("\n\n") {
            let event: String = buffer.drain(..end + 2).collect();
            if let Some(data) = event
                .lines()
                .find_map(|l| l.strip_prefix("data: ").or_else(|| l.strip_prefix("data:")))
                && !data.is_empty()
            {
                return serde_json::from_str(data).unwrap();
            }
            continue; // comment / keep-alive / legacy primer event
        }
        let frame = tokio::time::timeout(Duration::from_secs(5), body.frame())
            .await
            .expect("an SSE frame should arrive")
            .expect("stream open")
            .expect("frame ok");
        if let Some(data) = frame.data_ref() {
            buffer.push_str(&String::from_utf8_lossy(data));
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn elicitation_works_on_both_versions_over_http() {
    let app = router(Deleter.into_server().build(), HttpConfig::new());

    // Draft MRTR over plain POSTs.
    let resp = app
        .clone()
        .oneshot(post(draft_call(1, json!({})), DRAFT_HEADERS))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let first = body_json(resp).await;
    assert_eq!(first["result"]["resultType"], "input_required");

    let retry = draft_call(2, json!({ "inputResponses": { "confirm": accept() } }));
    let resp = app
        .clone()
        .oneshot(post(retry, DRAFT_HEADERS))
        .await
        .unwrap();
    let second = body_json(resp).await;
    assert_eq!(second["result"]["content"][0]["text"], "deleted /tmp/x");

    // Legacy: initialize mints the session…
    let init = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": { "elicitation": {} },
            "clientInfo": { "name": "legacy-http", "version": "1" },
        }
    });
    let resp = app.clone().oneshot(post(init, &[])).await.unwrap();
    let sid = resp.headers()["mcp-session-id"]
        .to_str()
        .unwrap()
        .to_owned();

    // …the call's own POST upgrades to a request-scoped SSE stream carrying
    // the server's elicitation request (transports spec SHOULD: request-
    // related messages ride the request's own response stream)…
    let call = json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": { "name": "delete", "arguments": { "path": "/tmp/z" } }
    });
    let resp = app
        .clone()
        .oneshot(post(call, &[("mcp-session-id", sid.as_str())]))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream"),
        "a mid-flight server request upgrades the POST response to SSE"
    );
    let mut stream = resp.into_body();
    let mut buffer = String::new();

    let elicit = next_sse_event(&mut stream, &mut buffer).await;
    assert_eq!(elicit["method"], "elicitation/create");
    assert_eq!(elicit["params"]["message"], "Delete /tmp/z?");

    // …the client POSTs the response back on its own request (202)…
    let answer = json!({ "jsonrpc": "2.0", "id": elicit["id"], "result": accept() });
    let resp = app
        .clone()
        .oneshot(post(answer, &[("mcp-session-id", sid.as_str())]))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    // …and the final response is the stream's last event; the response
    // terminates the stream.
    let done = next_sse_event(&mut stream, &mut buffer).await;
    assert_eq!(done["id"], 2);
    assert_eq!(done["result"]["content"][0]["text"], "deleted /tmp/z");
    let end = tokio::time::timeout(Duration::from_secs(5), stream.frame())
        .await
        .expect("the stream should end after the final response");
    assert!(end.is_none(), "no events after the final response");
}
