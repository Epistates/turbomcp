//! Cancellation, end to end across both halves.
//!
//! The receiving side has always been implemented: `notifications/cancelled`
//! fires the request's token and the dispatcher drops the handler future
//! without answering (cancellation spec: "stop processing … not send a response
//! for the cancelled request"). What these tests pin is that a real TurboMCP
//! *client* produces that notification when it abandons a request, so a server
//! stops working instead of computing an answer nobody will read.
//!
//! A handler cannot ask whether it was cancelled — dropping its future is the
//! mechanism — so the assertions are made from inside the tool with a drop
//! guard: it entered, it was dropped, and it never reached its last line.

#![cfg(feature = "client")]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::Map;
use tokio::io::{BufReader, split};
use turbomcp::client::{Client, ClientBuilder, ClientError, ConnectMode};
use turbomcp::prelude::*;
use turbomcp::{LegacySessionAdapter, SerdeJsonCodec, serve};
use turbomcp_transport_stdio::LineTransport;

/// What the tool got to before it stopped.
#[derive(Default)]
struct Marks {
    entered: AtomicBool,
    dropped: AtomicBool,
    finished: AtomicBool,
}

impl Marks {
    fn entered(&self) -> bool {
        self.entered.load(Ordering::SeqCst)
    }
    fn dropped(&self) -> bool {
        self.dropped.load(Ordering::SeqCst)
    }
    fn finished(&self) -> bool {
        self.finished.load(Ordering::SeqCst)
    }
}

/// Records that the handler future was dropped rather than run to completion.
struct DropMark(Arc<Marks>);

impl Drop for DropMark {
    fn drop(&mut self) {
        self.0.dropped.store(true, Ordering::SeqCst);
    }
}

#[derive(Clone)]
struct Slow {
    marks: Arc<Marks>,
}

#[server(name = "slow", version = "1.0.0")]
impl Slow {
    /// Sleep far longer than any caller will wait.
    #[tool(description = "Block until cancelled")]
    async fn block(&self) -> McpResult<String> {
        self.marks.entered.store(true, Ordering::SeqCst);
        // Dropped when the dispatcher drops this future — which is what
        // cancellation *is* on the server side.
        let _mark = DropMark(Arc::clone(&self.marks));
        tokio::time::sleep(Duration::from_secs(30)).await;
        self.marks.finished.store(true, Ordering::SeqCst);
        Ok("the caller waited it out".into())
    }
}

/// Spawn the server (the stack `run_stdio` wires) on one end of a duplex pipe
/// and connect a typed client with `request_timeout` on the other.
async fn connect(mode: ConnectMode, request_timeout: Duration) -> (Client, Arc<Marks>) {
    let marks = Arc::new(Marks::default());
    let server = Slow {
        marks: Arc::clone(&marks),
    };

    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let (s_rd, s_wr) = split(server_io);
    let transport = LineTransport::new(BufReader::new(s_rd), s_wr, SerdeJsonCodec);
    tokio::spawn(serve(
        transport,
        LegacySessionAdapter::new(server.into_server().build()),
    ));

    let (c_rd, c_wr) = split(client_io);
    let client_transport = LineTransport::new(BufReader::new(c_rd), c_wr, SerdeJsonCodec);
    let client = ClientBuilder::new("canceller", "1.0.0")
        .with_connect_mode(mode)
        .with_timeout(request_timeout)
        .connect(client_transport)
        .await
        .expect("handshake succeeds");
    (client, marks)
}

/// Give the notification a moment to cross the pipe and unwind the handler.
async fn settle(marks: &Marks) {
    for _ in 0..100 {
        if marks.dropped() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn assert_cancelled_server_side(marks: &Marks) {
    settle(marks).await;
    assert!(marks.entered(), "the tool should have started");
    assert!(
        marks.dropped(),
        "the handler future must be dropped, not left running for 30s"
    );
    assert!(
        !marks.finished(),
        "a cancelled handler must not run to completion"
    );
}

/// The client's own timeout elapses: it gives up *and* says so.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_timeout_stops_the_server_handler() {
    let (client, marks) = connect(ConnectMode::Modern, Duration::from_millis(200)).await;
    let result = client.call_tool("block", Map::new()).await;
    assert!(
        matches!(result, Err(ClientError::Timeout)),
        "expected Timeout, got {result:?}"
    );
    assert_cancelled_server_side(&marks).await;
}

/// The caller abandons the future instead — no error is ever returned, so the
/// only thing that can stop the server is the client noticing the drop.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropped_call_stops_the_server_handler() {
    let (client, marks) = connect(ConnectMode::Modern, Duration::from_secs(60)).await;
    let abandoned = tokio::time::timeout(
        Duration::from_millis(200),
        client.call_tool("block", Map::new()),
    )
    .await;
    assert!(
        abandoned.is_err(),
        "the caller's deadline should fire first"
    );
    assert_cancelled_server_side(&marks).await;
}

/// Cancellation is transport- and revision-independent: the legacy handshake
/// reaches the same in-flight registry.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_works_on_the_legacy_path_too() {
    let (client, marks) = connect(ConnectMode::Legacy, Duration::from_millis(200)).await;
    let result = client.call_tool("block", Map::new()).await;
    assert!(
        matches!(result, Err(ClientError::Timeout)),
        "expected Timeout, got {result:?}"
    );
    assert_cancelled_server_side(&marks).await;
}

/// A cancelled request must not also be answered. The client stays usable and
/// correlation is intact: the next call gets its own answer, not the ghost of
/// the abandoned one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancelled_request_is_never_answered() {
    let (client, marks) = connect(ConnectMode::Modern, Duration::from_millis(200)).await;
    assert!(client.call_tool("block", Map::new()).await.is_err());
    assert_cancelled_server_side(&marks).await;

    // `tools/list` is answered normally, proving the abandoned id left the
    // pending table without stranding the connection.
    let tools = client
        .list_tools(None)
        .await
        .expect("the client still works");
    assert_eq!(tools.tools.len(), 1);
}

// ---- Streamable HTTP ---------------------------------------------------------

/// Over HTTP the cancellation signal is the transport's, not a message:
/// "closing the SSE response stream is the cancellation signal. The server
/// **MUST** treat a client disconnect as cancellation of that request."
///
/// This is what makes `notifications/cancelled` unnecessary there — and it is
/// worth pinning, because it holds for a structural reason that is easy to
/// refactor away: the in-flight call future is *moved into* the response body,
/// so hyper dropping the body drops the call.
#[cfg(feature = "http")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_http_disconnect_cancels_the_request() {
    use std::net::{Ipv4Addr, SocketAddr};

    use turbomcp::CancellationToken;
    use turbomcp::http::{HttpConfig, ServeHttp};

    let marks = Arc::new(Marks::default());
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    drop(listener);

    let shutdown = CancellationToken::new();
    let server = Slow {
        marks: Arc::clone(&marks),
    };
    tokio::spawn(
        server
            .into_server()
            .run_http(addr, HttpConfig::new().with_shutdown(shutdown.clone())),
    );
    tokio::time::sleep(Duration::from_millis(150)).await;

    let url = format!("http://{addr}/mcp");
    let http = reqwest::Client::new();

    // Stateless (2026-07-28) needs no handshake: one POST is the whole call.
    let call = http
        .post(&url)
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2026-07-28")
        // SEP-2243: the draft mirrors the method (and the tool name) into
        // headers so a proxy can route without parsing the body.
        .header("mcp-method", "tools/call")
        .header("mcp-name", "block")
        .json(&serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {
                "name": "block", "arguments": {},
                "_meta": {
                    "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                    "io.modelcontextprotocol/clientCapabilities": {},
                },
            },
        }))
        .send();

    // Abandon the POST mid-flight: hyper drops the connection, which is the
    // signal. Nothing is sent to say so.
    let abandoned = tokio::time::timeout(Duration::from_millis(400), call).await;
    if let Ok(resp) = abandoned {
        let resp = resp.expect("request sent");
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        panic!("expected the call to block; got {status}: {body}");
    }

    settle(&marks).await;
    assert!(marks.entered(), "the tool should have started");
    assert!(
        marks.dropped(),
        "a client disconnect MUST cancel the request it was waiting on"
    );
    assert!(!marks.finished());
    shutdown.cancel();
}

/// The same abandonment, made through the typed client rather than by hand.
///
/// This is the case a user actually hits, and it is the one that pins the two
/// signals together: the client's `notifications/cancelled` arrives on a POST
/// of its own, and the server keys in-flight requests by the POST they came in
/// on, so the notification alone cannot reach the handler. What stops it is the
/// transport dropping the abandoned request's response body.
#[cfg(feature = "http")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_typed_http_client_timing_out_stops_the_server_handler() {
    use std::net::{Ipv4Addr, SocketAddr};

    use turbomcp::CancellationToken;
    use turbomcp::client::connect_http;
    use turbomcp::http::{HttpConfig, ServeHttp};

    let marks = Arc::new(Marks::default());
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    drop(listener);

    let shutdown = CancellationToken::new();
    let server = Slow {
        marks: Arc::clone(&marks),
    };
    tokio::spawn(
        server
            .into_server()
            .run_http(addr, HttpConfig::new().with_shutdown(shutdown.clone())),
    );
    tokio::time::sleep(Duration::from_millis(150)).await;

    let client = connect_http(
        ClientBuilder::new("canceller", "1.0.0")
            .with_connect_mode(ConnectMode::Modern)
            .with_timeout(Duration::from_millis(300)),
        &format!("http://{addr}/mcp"),
    )
    .await
    .expect("connect over http");

    let result = client.call_tool("block", Map::new()).await;
    assert!(
        matches!(result, Err(ClientError::Timeout)),
        "expected Timeout, got {result:?}"
    );

    assert_cancelled_server_side(&marks).await;
    shutdown.cancel();
}
