//! Client failure semantics against misbehaving servers: a dropped pipe fails
//! pending requests with `Closed` (promptly — no hang, no timeout wait), a
//! silent server yields `Timeout`, a response bearing an unknown id is ignored
//! without disturbing correlation, and a garbage frame ends the connection
//! (pending requests again fail `Closed`).
//!
//! Abandoning a request is also a wire event, not just a local one: whether the
//! client gave up on its own timeout or its caller dropped the future, the
//! server is told with `notifications/cancelled` so it can stop working.

#![cfg(feature = "client")]

use std::time::Duration;

use serde_json::{Map, Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, split};
use tokio::sync::mpsc;
use turbomcp::SerdeJsonCodec;
use turbomcp::client::{Client, ClientBuilder, ClientError, ConnectMode};
use turbomcp_transport_stdio::LineTransport;

/// What the scripted server does when the client's `tools/list` arrives.
#[derive(Clone, Copy)]
enum OnList {
    /// Close the connection without answering.
    DropPipe,
    /// Never answer (but keep the connection open).
    StaySilent,
    /// Write a response with an id nobody asked for, then the real answer.
    UnknownIdFirst,
    /// Write a non-JSON line.
    Garbage,
    /// Send the client a `ping` request before answering, then answer.
    PingFirst,
}

/// A hand-scripted draft server: answers `server/discover`, then applies
/// `behavior` to the first `tools/list`. Every frame the client sends is
/// mirrored to `seen`, so a test can assert on what went *out* as well as on
/// what the call returned.
fn spawn_scripted_server(
    server_io: tokio::io::DuplexStream,
    behavior: OnList,
    seen: Option<mpsc::UnboundedSender<Value>>,
) {
    tokio::spawn(async move {
        let (rd, mut wr) = split(server_io);
        let mut lines = BufReader::new(rd).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let frame: Value = serde_json::from_str(&line).expect("valid json from client");
            if let Some(seen) = &seen {
                let _ = seen.send(frame.clone());
            }
            // Only requests are answered. A notification carries no id, and a
            // *response* (the client answering something we asked) carries an
            // id but no method — neither wants a reply.
            let Some(method) = frame.get("method").and_then(Value::as_str) else {
                continue;
            };
            if frame.get("id").is_none() {
                continue;
            }
            let id = frame.get("id").cloned().unwrap_or(Value::Null);
            let result = match method {
                "server/discover" => json!({
                    "capabilities": { "tools": {} },
                    "supportedVersions": ["2026-07-28"],
                    "resultType": "complete", "cacheScope": "private", "ttlMs": 0
                }),
                // The stateful handshake, for the revisions that have one.
                "initialize" => json!({
                    "protocolVersion": "2025-11-25",
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "robustness-mock", "version": "1.0.0" },
                }),
                "tools/list" => match behavior {
                    OnList::PingFirst => {
                        let ping =
                            json!({ "jsonrpc": "2.0", "id": "srv-ping-1", "method": "ping" });
                        wr.write_all(format!("{ping}\n").as_bytes()).await.unwrap();
                        json!({ "tools": [] })
                    }
                    OnList::DropPipe => return, // drops rd + wr: EOF on both halves
                    OnList::StaySilent => continue,
                    OnList::Garbage => {
                        wr.write_all(b"!!! not json !!!\n").await.unwrap();
                        continue;
                    }
                    OnList::UnknownIdFirst => {
                        let stray = json!({
                            "jsonrpc": "2.0", "id": 424_242,
                            "result": { "should": "be ignored" }
                        });
                        wr.write_all(format!("{stray}\n").as_bytes()).await.unwrap();
                        json!({ "tools": [], "resultType": "complete",
                                "cacheScope": "private", "ttlMs": 0 })
                    }
                },
                other => panic!("unexpected method from client: {other:?}"),
            };
            let reply = json!({ "jsonrpc": "2.0", "id": id, "result": result });
            wr.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
        }
    });
}

async fn connect(behavior: OnList, request_timeout: Duration) -> Client {
    connect_observed(behavior, ConnectMode::Modern, request_timeout)
        .await
        .0
}

/// [`connect`], plus the stream of frames the client put on the wire.
async fn connect_observed(
    behavior: OnList,
    mode: ConnectMode,
    request_timeout: Duration,
) -> (Client, mpsc::UnboundedReceiver<Value>) {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let (seen_tx, seen_rx) = mpsc::unbounded_channel();
    spawn_scripted_server(server_io, behavior, Some(seen_tx));

    let (c_rd, c_wr) = split(client_io);
    let transport = LineTransport::new(BufReader::new(c_rd), c_wr, SerdeJsonCodec);
    let client = ClientBuilder::new("robustness", "1.0.0")
        .with_connect_mode(mode)
        .with_timeout(request_timeout)
        .connect(transport)
        .await
        .expect("handshake succeeds");
    (client, seen_rx)
}

/// The next frame the client puts on the wire that satisfies `want`, or `None`
/// if none arrives in time. Outbound frames are produced independently of the
/// call a test just awaited, so they have to be waited for rather than drained
/// — draining is a race, and it is a race that only loses on a slow machine.
async fn await_frame(
    seen: &mut mpsc::UnboundedReceiver<Value>,
    mut want: impl FnMut(&Value) -> bool,
) -> Option<Value> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let frame = tokio::time::timeout_at(deadline, seen.recv())
            .await
            .ok()??;
        if want(&frame) {
            return Some(frame);
        }
    }
}

/// The params of the `notifications/cancelled` naming a request the client
/// issued for `method`.
async fn await_cancellation_of(
    seen: &mut mpsc::UnboundedReceiver<Value>,
    method: &str,
) -> Option<Value> {
    let mut ids = Vec::new();
    let frame = await_frame(seen, |f| {
        match f.get("method").and_then(Value::as_str) {
            // Remember the id, so the cancellation can be tied back to it.
            Some(m) if m == method => {
                ids.push(f.get("id").cloned().unwrap_or(Value::Null));
                false
            }
            Some("notifications/cancelled") => ids.contains(&f["params"]["requestId"]),
            _ => false,
        }
    })
    .await?;
    Some(frame.get("params").cloned().unwrap_or(Value::Null))
}

/// The whole point of the actor's exit-drain: a caller blocked on a request
/// must see `Closed` as soon as the connection dies — not hang, and not wait
/// out the (long) request timeout.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropped_pipe_fails_pending_requests_closed_promptly() {
    let client = connect(OnList::DropPipe, Duration::from_secs(60)).await;
    let result = tokio::time::timeout(Duration::from_secs(2), client.list_tools(None))
        .await
        .expect("failure arrives promptly, not after the 60s request timeout");
    assert!(
        matches!(result, Err(ClientError::Closed)),
        "expected Closed, got {result:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn silent_server_yields_timeout() {
    let client = connect(OnList::StaySilent, Duration::from_millis(150)).await;
    let result = client.list_tools(None).await;
    assert!(
        matches!(result, Err(ClientError::Timeout)),
        "expected Timeout, got {result:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_response_id_is_ignored_and_correlation_survives() {
    let client = connect(OnList::UnknownIdFirst, Duration::from_secs(5)).await;
    let tools = client
        .list_tools(None)
        .await
        .expect("the stray response must not disturb the real one");
    assert!(tools.tools.is_empty());
}

/// A frame that fails to decode ends the connection (the transport is the
/// trust boundary — there is no resync on a corrupted stream) and pending
/// requests fail `Closed` rather than hanging.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn garbage_frame_ends_the_connection_and_fails_pending() {
    let client = connect(OnList::Garbage, Duration::from_secs(60)).await;
    let result = tokio::time::timeout(Duration::from_secs(2), client.list_tools(None))
        .await
        .expect("failure arrives promptly");
    assert!(
        matches!(result, Err(ClientError::Closed)),
        "expected Closed, got {result:?}"
    );
    // The client object itself stays safe to use: further calls fail cleanly.
    let again = client.request("tools/list", Map::new()).await;
    assert!(again.is_err());
}

/// Giving up locally is only half of it. The server is still working on a
/// request whose answer nobody will ever read, and the only way it learns
/// otherwise is `notifications/cancelled` (cancellation spec: receivers SHOULD
/// stop processing and free resources).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn timing_out_tells_the_server_to_stop() {
    let (client, mut seen) = connect_observed(
        OnList::StaySilent,
        ConnectMode::Modern,
        Duration::from_millis(150),
    )
    .await;
    let result = client.list_tools(None).await;
    assert!(
        matches!(result, Err(ClientError::Timeout)),
        "expected Timeout, got {result:?}"
    );

    let params = await_cancellation_of(&mut seen, "tools/list")
        .await
        .expect("the abandoned tools/list must be cancelled on the wire");
    assert_eq!(
        params["reason"], "the client's request timeout elapsed",
        "the reason is for the server's log; say which side gave up and why"
    );
}

/// The same obligation, reached the other way: a caller that races `request`
/// against its own timeout (or a `select!`) drops the future without any error
/// ever surfacing. The request is just as abandoned, so it is cancelled just
/// the same — and the pending entry has to go with it, or a long-lived client
/// leaks one per abandoned call.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_the_request_future_tells_the_server_to_stop() {
    let (client, mut seen) = connect_observed(
        OnList::StaySilent,
        ConnectMode::Modern,
        Duration::from_secs(60),
    )
    .await;

    // The caller's own deadline fires long before the client's 60s timeout, so
    // the future is dropped mid-flight rather than resolving to `Timeout`.
    let abandoned = tokio::time::timeout(Duration::from_millis(150), client.list_tools(None)).await;
    assert!(
        abandoned.is_err(),
        "the caller's deadline should fire first"
    );

    let params = await_cancellation_of(&mut seen, "tools/list")
        .await
        .expect("a dropped request future must still be cancelled on the wire");
    assert_eq!(params["reason"], "the caller dropped the request");
}

/// Ping is bidirectional and mandatory in both directions: "the receiver MUST
/// respond promptly with an empty response". It is also the one server→client
/// request that has nothing to do with the application, so it must be answered
/// by a client that installed no [`ClientHandler`] at all — which is exactly
/// the client this test builds.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_inbound_ping_is_answered_without_a_handler() {
    let (client, mut seen) = connect_observed(
        OnList::PingFirst,
        ConnectMode::Legacy,
        Duration::from_secs(5),
    )
    .await;

    let tools = client.list_tools(None).await.expect("the list is answered");
    assert!(tools.tools.is_empty());

    // The reply is written independently of the `tools/list` answer, so wait
    // for it rather than draining whatever has arrived by now.
    let reply = await_frame(&mut seen, |f| {
        f.get("id").and_then(Value::as_str) == Some("srv-ping-1")
    })
    .await
    .expect("the client must answer the server's ping");
    assert_eq!(
        reply["result"],
        json!({}),
        "ping is answered with an empty result, not an error: {reply}"
    );
}

/// Admission closes before transport teardown, even when teardown blocks.
#[tokio::test]
async fn requests_during_transport_teardown_fail_closed_promptly() {
    use turbomcp::{JsonRpcMessage, Transport};
    struct ClosingTransport {
        entered: tokio::sync::oneshot::Sender<()>,
        release: tokio::sync::oneshot::Receiver<()>,
    }
    impl Transport for ClosingTransport {
        type Error = std::io::Error;
        async fn send(&mut self, _: JsonRpcMessage) -> Result<(), Self::Error> {
            Ok(())
        }
        async fn recv(&mut self) -> Result<Option<JsonRpcMessage>, Self::Error> {
            Err(std::io::Error::other("peer disconnected"))
        }
        async fn close(self) -> Result<(), Self::Error> {
            let _ = self.entered.send(());
            let _ = self.release.await;
            Ok(())
        }
    }
    let (entered, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = tokio::sync::oneshot::channel();
    let connection = turbomcp::client::Connection::with_timeout(
        ClosingTransport {
            entered,
            release: wait,
        },
        Duration::from_secs(60),
    );
    tokio::time::timeout(Duration::from_secs(1), ready)
        .await
        .unwrap()
        .unwrap();
    let result =
        tokio::time::timeout(Duration::from_secs(1), connection.request("ping", None)).await;
    let _ = release.send(());
    assert!(
        matches!(result, Ok(Err(ClientError::Closed))),
        "late request must fail immediately: {result:?}"
    );
    connection.close().await;
}
