//! WebSocket as a route on the HTTP endpoint: the typed client end to end,
//! the endpoint's guards applied before the upgrade, the `mcp` subprotocol,
//! graceful drain, credential expiry and the connection cap.
#![cfg(all(feature = "websocket", feature = "client"))]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::{Map, Value, json};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::{Error as WsProtocolError, Message};
use turbomcp::client::{ClientBuilder, ConnectMode, WebSocketClientTransport, connect_websocket};
use turbomcp::http::{Http, HttpConfig, WebSocketConfig};
use turbomcp::prelude::*;
use turbomcp::{AuthDecision, AuthFuture, CancellationToken, HttpAuthenticator, Identity};

/// Set when `slow` finishes, so a test can tell whether serving returned
/// before or after it.
static SLOW_FINISHED: AtomicBool = AtomicBool::new(false);

#[derive(Clone)]
struct Srv;

#[server(name = "ws-srv", version = "1.0.0")]
impl Srv {
    /// Echo the message back.
    #[tool]
    async fn echo(&self, msg: String) -> String {
        msg
    }

    /// Who the connection authenticated as.
    #[tool]
    async fn whoami(&self, ctx: &CallToolContext) -> String {
        ctx.base
            .identity
            .subject()
            .unwrap_or("anonymous")
            .to_owned()
    }

    /// Finish after a while.
    #[tool]
    async fn slow(&self) -> String {
        tokio::time::sleep(Duration::from_millis(300)).await;
        SLOW_FINISHED.store(true, Ordering::SeqCst);
        "done".into()
    }
}

struct Running {
    url: String,
    shutdown: CancellationToken,
    serving: tokio::task::JoinHandle<Result<(), turbomcp::ProtocolError>>,
}

async fn spawn(config: HttpConfig) -> Running {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let shutdown = CancellationToken::new();
    let config = config.with_shutdown(shutdown.clone());
    let serving = tokio::spawn(
        Srv.into_server()
            .serve(Http::listener(listener).config(config)),
    );
    Running {
        url: format!("ws://{addr}/ws"),
        shutdown,
        serving,
    }
}

fn with_ws() -> HttpConfig {
    HttpConfig::new().with_websocket(WebSocketConfig::new("/ws"))
}

fn args(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect()
}

fn text(result: &neutral::CallToolResult) -> &str {
    match &result.content[0] {
        neutral::Content::Text { text, .. } => text,
        other => panic!("expected text, got {other:?}"),
    }
}

/// The status a refused upgrade answered with.
async fn refused(request: impl IntoClientRequest + Unpin) -> u16 {
    match tokio_tungstenite::connect_async(request).await {
        Err(WsProtocolError::Http(response)) => response.status().as_u16(),
        Err(other) => panic!("expected an HTTP refusal, got {other}"),
        Ok(_) => panic!("the upgrade was accepted"),
    }
}

/// A stateful client works too: each connection is its own session.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_typed_client_works_in_both_modes() {
    let server = spawn(with_ws()).await;
    for mode in [ConnectMode::Modern, ConnectMode::Legacy] {
        let client = ClientBuilder::new("ws-typed", "1.0.0")
            .with_connect_mode(mode)
            .connect(connect_websocket(&server.url).await.unwrap())
            .await
            .unwrap_or_else(|e| panic!("{mode:?} handshake: {e}"));
        let result = client
            .call_tool("echo", args(&[("msg", json!("over-ws"))]))
            .await
            .unwrap();
        assert_eq!(text(&result), "over-ws", "{mode:?}");
        client.close().await;
    }
}

/// Browsers and the official SDKs' clients ask for `mcp`, and fail the
/// connection when the server doesn't select it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_mcp_subprotocol_is_selected() {
    let server = spawn(with_ws()).await;
    let mut request = server.url.as_str().into_client_request().unwrap();
    request
        .headers_mut()
        .insert("sec-websocket-protocol", "mcp".parse().unwrap());
    let (_socket, response) = tokio_tungstenite::connect_async(request).await.unwrap();
    assert_eq!(response.headers()["sec-websocket-protocol"], "mcp");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_browser_origin_is_refused_before_the_upgrade() {
    let server = spawn(with_ws()).await;
    let mut request = server.url.as_str().into_client_request().unwrap();
    request
        .headers_mut()
        .insert("origin", "https://evil.example".parse().unwrap());
    assert_eq!(refused(request).await, 403);
}

/// Allows `Bearer good` as `tester`, with a token that expires `ttl` seconds
/// from now.
struct Tokens {
    ttl: Option<u64>,
}

impl HttpAuthenticator for Tokens {
    fn authenticate<'a>(&'a self, authorization: Option<&'a str>) -> AuthFuture<'a> {
        Box::pin(async move {
            if authorization != Some("Bearer good") {
                return AuthDecision::Challenge {
                    status: 401,
                    www_authenticate: "Bearer realm=\"ws\"".to_owned(),
                };
            }
            let mut claims = Map::new();
            if let Some(ttl) = self.ttl {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                claims.insert("exp".into(), json!(now + ttl));
            }
            AuthDecision::Allow(Identity::Bearer {
                sub: "tester".into(),
                claims,
            })
        })
    }

    fn resource_metadata(&self) -> Value {
        json!({})
    }
}

fn authorized(url: &str) -> tokio_tungstenite::tungstenite::handshake::client::Request {
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("authorization", "Bearer good".parse().unwrap());
    request
}

/// A bad token is a `401` with its challenge, before the upgrade: the
/// standalone WebSocket server could only close with `1008` after the `101`,
/// which gave a client no way to discover the authorization server.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn authentication_happens_before_the_upgrade() {
    let server = spawn(with_ws().with_authenticator(Arc::new(Tokens { ttl: None }))).await;
    assert_eq!(refused(server.url.as_str()).await, 401);

    let client = ClientBuilder::new("ws-auth", "1.0.0")
        .connect(
            WebSocketClientTransport::connect(authorized(&server.url))
                .await
                .unwrap(),
        )
        .await
        .unwrap();
    let who = client.call_tool("whoami", Map::new()).await.unwrap();
    assert_eq!(text(&who), "tester");
}

/// An `http::Request` built by hand carries only what its author put on it;
/// the transport adds the upgrade's own headers rather than fail it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_request_built_by_hand_upgrades() {
    let server = spawn(with_ws().with_authenticator(Arc::new(Tokens { ttl: None }))).await;
    let request = tokio_tungstenite::tungstenite::http::Request::builder()
        .uri(server.url.as_str())
        .header("authorization", "Bearer good")
        .body(())
        .unwrap();
    let client = ClientBuilder::new("ws-hand", "1.0.0")
        .connect(WebSocketClientTransport::connect(request).await.unwrap())
        .await
        .unwrap();
    let who = client.call_tool("whoami", Map::new()).await.unwrap();
    assert_eq!(text(&who), "tester");
}

/// A token that expires stops authorizing the connection it opened: the
/// server closes it with `1008`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_expired_credential_closes_the_connection() {
    let server = spawn(with_ws().with_authenticator(Arc::new(Tokens { ttl: Some(1) }))).await;
    let (mut socket, _) = tokio_tungstenite::connect_async(authorized(&server.url))
        .await
        .unwrap();
    let close = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Close(frame))) => return frame,
                Some(Ok(_)) => {}
                other => panic!("expected a close frame, got {other:?}"),
            }
        }
    })
    .await
    .expect("the connection is closed when the credential expires");
    assert_eq!(close.expect("a close frame").code, CloseCode::Policy);
}

/// One bad frame costs one frame: it is answered, and the connection goes on.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_malformed_frame_is_answered_and_the_connection_survives() {
    let server = spawn(with_ws()).await;
    let (mut socket, _) = tokio_tungstenite::connect_async(server.url.as_str())
        .await
        .unwrap();
    socket
        .send(Message::text("!!! not json !!!"))
        .await
        .unwrap();
    let reply = next_json(&mut socket).await;
    assert_eq!(reply["error"]["code"], -32700);

    let call = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {
            "name": "echo", "arguments": { "msg": "still here" },
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
            },
        },
    });
    socket.send(Message::text(call.to_string())).await.unwrap();
    let reply = next_json(&mut socket).await;
    assert_eq!(reply["result"]["content"][0]["text"], "still here");
}

async fn next_json<S>(socket: &mut S) -> Value
where
    S: futures::Stream<Item = Result<Message, WsProtocolError>> + Unpin,
{
    loop {
        match tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("a reply")
        {
            Some(Ok(Message::Text(text))) => return serde_json::from_str(text.as_str()).unwrap(),
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
            other => panic!("expected a text frame, got {other:?}"),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_message_over_the_body_limit_ends_the_connection() {
    let server = spawn(with_ws().max_body_bytes(1024)).await;
    let (mut socket, _) = tokio_tungstenite::connect_async(server.url.as_str())
        .await
        .unwrap();
    socket
        .send(Message::text("x".repeat(4 * 1024)))
        .await
        .unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                other => return other,
            }
        }
    })
    .await
    .expect("the server ends the connection");
    assert!(
        !matches!(outcome, Some(Ok(Message::Text(_)))),
        "the oversized message was served: {outcome:?}"
    );
}

/// Shutdown waits for a connection's in-flight call to finish and reach the
/// client. The standalone server returned at once, and the process exited
/// under its draining connections.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_drains_in_flight_calls() {
    let server = spawn(with_ws()).await;
    let client = ClientBuilder::new("ws-drain", "1.0.0")
        .connect(connect_websocket(&server.url).await.unwrap())
        .await
        .unwrap();
    let call = tokio::spawn(async move { client.call_tool("slow", Map::new()).await });
    tokio::time::sleep(Duration::from_millis(50)).await;

    server.shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(5), server.serving)
        .await
        .expect("serving returns once drained")
        .unwrap()
        .unwrap();
    assert!(
        SLOW_FINISHED.load(Ordering::SeqCst),
        "serving returned before the in-flight call finished"
    );
    let result = call.await.unwrap().expect("the in-flight call finished");
    assert_eq!(text(&result), "done");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connections_past_the_cap_are_refused() {
    let server =
        spawn(HttpConfig::new().with_websocket(WebSocketConfig::new("/ws").max_connections(1)))
            .await;
    let (_first, _) = tokio_tungstenite::connect_async(server.url.as_str())
        .await
        .unwrap();
    assert_eq!(refused(server.url.as_str()).await, 503);
}
