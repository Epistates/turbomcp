//! Per-call input handlers: a server's request for input reaches the handler
//! of the call it belongs to (SEP-2260), by whatever the revision and
//! transport say about which call that is, and never a guess.

#![cfg(feature = "client")]

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Map, Value, json};
use tokio::io::{BufReader, split};
use tokio::sync::Barrier;
use turbomcp::SerdeJsonCodec;
use turbomcp::client::{
    CallOptions, Client, ClientBuilder, ConnectMode, ElicitationHandler, async_trait,
};
use turbomcp::prelude::*;
use turbomcp_service::io::LineTransport;

#[derive(Clone)]
struct Desk {
    /// Holds concurrent calls until all of them are in flight.
    together: Arc<Barrier>,
}

impl Desk {
    fn new(calls: usize) -> Self {
        Self {
            together: Arc::new(Barrier::new(calls)),
        }
    }
}

async fn who(ctx: &CallToolContext) -> McpResult<String> {
    let outcome = ctx
        .client
        .elicit(
            "who",
            neutral::ElicitParams::new(
                "Who is answering?",
                json!({ "type": "object", "properties": { "who": { "type": "string" } } }),
            ),
        )
        .await?;
    Ok(outcome
        .content
        .get("who")
        .and_then(Value::as_str)
        .unwrap_or("nobody")
        .to_owned())
}

#[server(name = "desk", version = "1.0.0")]
impl Desk {
    /// Ask who answers.
    #[tool]
    async fn ask(&self, ctx: &CallToolContext) -> McpResult<String> {
        who(ctx).await
    }

    /// Ask who answers while every concurrent call is in flight.
    #[tool]
    async fn ask_together(&self, ctx: &CallToolContext) -> McpResult<String> {
        self.together.wait().await;
        let who = who(ctx).await;
        // None ends before all have asked: one call alone in flight is
        // unambiguous.
        self.together.wait().await;
        who
    }

    /// Ask who answers, as a task, once every concurrent task is running.
    #[tool(task)]
    async fn ask_as_task(&self, ctx: &CallToolContext) -> McpResult<String> {
        self.together.wait().await;
        who(ctx).await
    }
}

/// Answers every elicitation with its name.
#[derive(Clone)]
struct Named(&'static str);

#[async_trait]
impl ElicitationHandler for Named {
    async fn elicit(&self, _request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        let mut content = Map::new();
        content.insert("who".into(), json!(self.0));
        neutral::ElicitOutcome::new(neutral::ElicitAction::Accept, content)
    }
}

fn builder(mode: ConnectMode) -> ClientBuilder {
    ClientBuilder::new("asker", "1.0.0")
        .with_connect_mode(mode)
        .with_elicitation(Named("global"))
}

/// `desk` over a newline-delimited pipe: a transport that says nothing about
/// which call a server request belongs to.
async fn over_stdio(desk: Desk, mode: ConnectMode) -> Client {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let (s_rd, s_wr) = split(server_io);
    let transport = LineTransport::new(BufReader::new(s_rd), s_wr, SerdeJsonCodec);
    tokio::spawn(desk.into_server().with_tasks().serve(transport));
    let (c_rd, c_wr) = split(client_io);
    builder(mode)
        .connect(LineTransport::new(
            BufReader::new(c_rd),
            c_wr,
            SerdeJsonCodec,
        ))
        .await
        .expect("handshake")
}

fn text(result: &neutral::CallToolResult) -> String {
    result.text_content().expect("text")
}

fn answered_by(name: &'static str) -> CallOptions {
    CallOptions::new().with_elicitation(Named(name))
}

/// Call `tool` twice at once, with `a` and with `b`.
async fn both(client: &Client, tool: &str, a: CallOptions, b: CallOptions) -> (String, String) {
    let (ra, rb) = tokio::join!(
        client.call_tool_with(tool, Map::new(), &a),
        client.call_tool_with(tool, Map::new(), &b),
    );
    (text(&ra.unwrap()), text(&rb.unwrap()))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_with_its_own_handler_gets_its_questions_on_every_revision() {
    for mode in [ConnectMode::Modern, ConnectMode::Legacy] {
        let client = over_stdio(Desk::new(1), mode).await;
        let own = client
            .call_tool_with("ask", Map::new(), &answered_by("call"))
            .await
            .unwrap();
        assert_eq!(text(&own), "call", "{mode:?}");
        let plain = client.call_tool("ask", Map::new()).await.unwrap();
        assert_eq!(text(&plain), "global", "{mode:?}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mrtr_attributes_concurrent_calls_structurally() {
    let client = over_stdio(Desk::new(2), ConnectMode::Modern).await;
    let (a, b) = both(&client, "ask_together", answered_by("a"), answered_by("b")).await;
    assert_eq!(a, "a");
    assert_eq!(b, "b");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_calls_on_a_transport_that_cannot_tell_get_the_clients_handler() {
    // Over stdio on `2025-11-25` an `elicitation/create` names no call. With
    // two in flight, handing it to either call's handler would be a guess.
    let client = over_stdio(Desk::new(2), ConnectMode::Legacy).await;
    let (a, b) = both(&client, "ask_together", answered_by("a"), answered_by("b")).await;
    assert_eq!(a, "global");
    assert_eq!(b, "global");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_task_routes_its_questions_by_related_task() {
    let client = over_stdio(Desk::new(2), ConnectMode::Legacy).await;
    let (a, b) = both(
        &client,
        "ask_as_task",
        answered_by("a").task(Some(60_000)),
        answered_by("b").task(Some(60_000)),
    )
    .await;
    assert_eq!(a, "a");
    assert_eq!(b, "b");
}

#[cfg(feature = "http")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn streamable_http_attributes_concurrent_calls_by_their_stream() {
    use turbomcp::http::{Http, HttpConfig};

    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(
        Desk::new(2)
            .into_server()
            .serve(Http::listener(listener).config(HttpConfig::new())),
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    let client =
        turbomcp::client::connect_http(builder(ConnectMode::Legacy), format!("http://{addr}/mcp"))
            .await
            .expect("handshake");
    let (a, b) = both(&client, "ask_together", answered_by("a"), answered_by("b")).await;
    assert_eq!(a, "a");
    assert_eq!(b, "b");
}
