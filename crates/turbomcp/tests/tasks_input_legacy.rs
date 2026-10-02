//! Mid-task client input on `2025-11-25` (tasks.mdx §Input Required Status):
//! a tool running as a task elicits while it executes, the task reads
//! `input_required`, the server sends the request on a stream the client
//! holds, and the client's answer resumes the handler. Driven end to end by
//! the real client over stdio and over Streamable HTTP.

#![cfg(feature = "client")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value, json};
use tokio::io::{BufReader, split};
use turbomcp::client::{Client, ClientBuilder, ConnectMode, ElicitationHandler, async_trait};
use turbomcp::prelude::*;
use turbomcp::{JsonRpcMessage, McpRequest, SerdeJsonCodec};
use turbomcp_service::io::LineTransport;

#[derive(Clone)]
struct Desk;

#[server(name = "desk", version = "1.0.0")]
impl Desk {
    /// Sign a document once the user confirms.
    #[tool(task)]
    async fn sign(&self, ctx: &CallToolContext, document: String) -> McpResult<String> {
        assert!(ctx.task.is_task(), "runs as a task");
        let outcome = ctx
            .client
            .elicit(
                "confirm_sign",
                neutral::ElicitParams::new(
                    format!("Sign {document}?"),
                    json!({
                        "type": "object",
                        "properties": { "ok": { "type": "boolean" } },
                    }),
                ),
            )
            .await?;
        Ok(if outcome.accepted() {
            format!("signed {document}")
        } else {
            format!("left {document} unsigned")
        })
    }
}

/// Accepts every elicitation, counting them.
#[derive(Clone, Default)]
struct Confirm(Arc<AtomicUsize>);

#[async_trait]
impl ElicitationHandler for Confirm {
    async fn elicit(&self, request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        assert_eq!(request.message, "Sign lease.pdf?");
        self.0.fetch_add(1, Ordering::SeqCst);
        let mut content = Map::new();
        content.insert("ok".into(), Value::Bool(true));
        neutral::ElicitOutcome::new(neutral::ElicitAction::Accept, content)
    }
}

fn client(confirm: &Confirm) -> ClientBuilder {
    ClientBuilder::new("desk-client", "1.0.0")
        .with_connect_mode(ConnectMode::Legacy)
        .with_elicitation(confirm.clone())
}

async fn sign(client: &Client) -> String {
    let mut args = Map::new();
    args.insert("document".into(), json!("lease.pdf"));
    let result = client
        .call_tool_task("sign", args, Some(60_000))
        .await
        .expect("the task completes");
    match &result.content[0] {
        neutral::Content::Text { text, .. } => text.clone(),
        other => panic!("unexpected content {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_task_elicits_mid_execution_over_stdio() {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let (s_rd, s_wr) = split(server_io);
    let transport = LineTransport::new(BufReader::new(s_rd), s_wr, SerdeJsonCodec);
    // Every response the client sends the server, to check what it carries.
    let answers = Arc::new(Mutex::new(Vec::new()));
    let record = {
        let answers = Arc::clone(&answers);
        move |req: McpRequest| {
            if let JsonRpcMessage::Response(r) = &req.message {
                answers.lock().unwrap().push(r.result.clone());
            }
            req
        }
    };
    tokio::spawn(
        Desk.into_server()
            .with_tasks()
            .layer(tower::util::MapRequestLayer::new(record))
            .serve(transport),
    );

    let confirm = Confirm::default();
    let (c_rd, c_wr) = split(client_io);
    let client = client(&confirm)
        .connect(LineTransport::new(
            BufReader::new(c_rd),
            c_wr,
            SerdeJsonCodec,
        ))
        .await
        .expect("handshake");
    assert_eq!(sign(&client).await, "signed lease.pdf");
    assert_eq!(confirm.0.load(Ordering::SeqCst), 1, "asked exactly once");
    // "All requests, notifications, and responses related to a task MUST
    // include the `io.modelcontextprotocol/related-task` key".
    let answers = answers.lock().unwrap();
    let [Some(answer)] = answers.as_slice() else {
        panic!("one answer, got {answers:?}");
    };
    assert!(
        answer["_meta"]["io.modelcontextprotocol/related-task"]["taskId"].is_string(),
        "{answer}"
    );
}

#[cfg(feature = "http")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_task_elicits_mid_execution_over_http() {
    use turbomcp::client::connect_http;
    use turbomcp::http::{Http, HttpConfig};

    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let shutdown = turbomcp::CancellationToken::new();
    tokio::spawn(
        Desk.into_server().with_tasks().serve(
            Http::listener(listener).config(HttpConfig::new().with_shutdown(shutdown.clone())),
        ),
    );

    let confirm = Confirm::default();
    let client = connect_http(client(&confirm), &url).await.expect("connect");
    assert_eq!(sign(&client).await, "signed lease.pdf");
    assert_eq!(confirm.0.load(Ordering::SeqCst), 1, "asked exactly once");
    shutdown.cancel();
}
