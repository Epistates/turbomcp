//! Production guards on the WebSocket server: Origin policy at the upgrade,
//! bearer auth right after it (close 1008 on rejection), the authenticated
//! principal attached to every inbound request, and the inbound
//! message-size cap.

use std::task::{Context, Poll};

use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tower::Service;
use turbomcp_core::codec::DefaultCodec;
use turbomcp_core::{Identity, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, McpRequest};
use turbomcp_service::{AuthDecision, AuthFuture, HttpAuthenticator, ProtocolError, Transport};
use turbomcp_transport_ws::{WebSocketTransport, WsConfig, serve_websocket_with};

/// Answers every request with the subject of the identity attached to it (so
/// the test can observe what the transport established).
#[derive(Clone)]
struct IdentityEcho;

impl Service<McpRequest> for IdentityEcho {
    type Response = Option<JsonRpcMessage>;
    type Error = ProtocolError;
    type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: McpRequest) -> Self::Future {
        let JsonRpcMessage::Request(req) = request.message else {
            return std::future::ready(Ok(None));
        };
        let subject = request
            .extensions
            .get::<Identity>()
            .and_then(Identity::subject)
            .map(str::to_owned);
        let resp = JsonRpcResponse::success(req.id, json!({ "identity": { "sub": subject } }));
        std::future::ready(Ok(Some(resp.into())))
    }
}

/// Allows exactly `Bearer good`, as subject `tester`.
struct GoodToken;

impl HttpAuthenticator for GoodToken {
    fn authenticate<'a>(&'a self, authorization: Option<&'a str>) -> AuthFuture<'a> {
        Box::pin(async move {
            if authorization == Some("Bearer good") {
                AuthDecision::Allow(Identity::Bearer {
                    sub: "tester".into(),
                    claims: serde_json::Map::new(),
                })
            } else {
                AuthDecision::Challenge {
                    status: 401,
                    www_authenticate: "Bearer".to_owned(),
                }
            }
        })
    }

    fn resource_metadata(&self) -> Value {
        json!({})
    }
}

async fn spawn_server(config: WsConfig) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = serve_websocket_with(listener, || IdentityEcho, config).await;
    });
    addr
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn browser_origin_is_rejected_at_the_upgrade() {
    let addr = spawn_server(WsConfig::new()).await;

    let mut req = format!("ws://{addr}").into_client_request().unwrap();
    req.headers_mut()
        .insert("origin", "https://evil.example".parse().unwrap());
    let result = tokio_tungstenite::connect_async(req).await;
    assert!(result.is_err(), "default policy rejects browser origins");

    // An allowlisted origin connects.
    let addr = spawn_server(WsConfig::new().allow_origin("https://app.example")).await;
    let mut req = format!("ws://{addr}").into_client_request().unwrap();
    req.headers_mut()
        .insert("origin", "https://app.example".parse().unwrap());
    assert!(tokio_tungstenite::connect_async(req).await.is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn oversized_frames_end_the_connection() {
    let addr = spawn_server(WsConfig::new().max_message_bytes(1024)).await;

    // A frame within the cap is served normally.
    let req = format!("ws://{addr}").into_client_request().unwrap();
    let (stream, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    let mut client = WebSocketTransport::new(stream, DefaultCodec::default());
    client
        .send(JsonRpcMessage::Request(JsonRpcRequest::new(
            1, "small", None,
        )))
        .await
        .unwrap();
    assert!(
        client.recv().await.unwrap().is_some(),
        "within-cap frame is answered"
    );

    // A frame over the cap must NOT be answered: the server rejects it at the
    // socket layer and the connection ends (recv sees close/error, never a
    // response).
    client
        .send(JsonRpcMessage::Request(JsonRpcRequest::new(
            2,
            "big",
            Some(json!({ "blob": "x".repeat(4 * 1024) })),
        )))
        .await
        .unwrap();
    match client.recv().await {
        Ok(None) | Err(_) => {} // closed — the guard held
        Ok(Some(msg)) => panic!("oversized frame was served: {msg:?}"),
    }
}

/// A text frame that isn't JSON is answered with a Parse error and costs
/// nothing else: WebSocket framing delivers the next message intact, so the
/// same connection goes on to serve a well-formed request.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_malformed_text_frame_is_answered_and_the_connection_survives() {
    use futures::SinkExt;
    use tokio_tungstenite::tungstenite::Message;

    let addr = spawn_server(WsConfig::new()).await;
    let req = format!("ws://{addr}").into_client_request().unwrap();
    let (mut stream, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    stream
        .send(Message::text("!!! not json !!!"))
        .await
        .unwrap();
    let mut client = WebSocketTransport::new(stream, DefaultCodec::default());
    let Some(JsonRpcMessage::Response(parse)) = client.recv().await.unwrap() else {
        panic!("expected the parse error");
    };
    assert!(parse.id.is_none());
    assert_eq!(parse.error.expect("an error").code, -32700);

    client
        .send(JsonRpcMessage::Request(JsonRpcRequest::new(1, "ok", None)))
        .await
        .unwrap();
    assert!(client.recv().await.unwrap().is_some(), "still serving");
}

/// Binary frames carry the same JSON payload as text frames and decode
/// identically (the recv path accepts both).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn binary_frames_decode_like_text() {
    use futures::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let addr = spawn_server(WsConfig::new()).await;
    let req = format!("ws://{addr}").into_client_request().unwrap();
    let (mut stream, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    let frame = serde_json::to_vec(&JsonRpcMessage::Request(JsonRpcRequest::new(
        7, "binary", None,
    )))
    .unwrap();
    stream.send(Message::binary(frame)).await.unwrap();
    loop {
        match stream.next().await.expect("a reply").expect("frame") {
            Message::Text(t) => {
                let v: Value = serde_json::from_str(t.as_str()).unwrap();
                assert_eq!(v["id"], 7, "the binary request was served: {v}");
                break;
            }
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("expected a text reply, got {other:?}"),
        }
    }
}

/// The auth-rejection close is a real policy-violation close frame — code
/// `1008` with the reason — not a bare TCP drop.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auth_rejection_closes_with_policy_code_1008() {
    use futures::StreamExt;
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

    let config = WsConfig::new().with_authenticator(std::sync::Arc::new(GoodToken));
    let addr = spawn_server(config).await;

    let mut req = format!("ws://{addr}").into_client_request().unwrap();
    req.headers_mut()
        .insert("authorization", "Bearer wrong".parse().unwrap());
    let (mut stream, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    loop {
        match stream.next().await.expect("a close frame").expect("frame") {
            Message::Close(Some(frame)) => {
                assert_eq!(frame.code, CloseCode::Policy, "1008 policy violation");
                break;
            }
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("expected Close(1008), got {other:?}"),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bearer_auth_gates_the_connection_and_stamps_identity() {
    let config = WsConfig::new().with_authenticator(std::sync::Arc::new(GoodToken));
    let addr = spawn_server(config).await;

    // Wrong token: the upgrade completes (WS has no post-upgrade 401) but the
    // server immediately closes with policy violation — recv sees end-of-stream.
    let mut req = format!("ws://{addr}").into_client_request().unwrap();
    req.headers_mut()
        .insert("authorization", "Bearer wrong".parse().unwrap());
    let (stream, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    let mut rejected = WebSocketTransport::new(stream, DefaultCodec::default());
    assert!(
        rejected.recv().await.unwrap().is_none(),
        "rejected connection closes without serving"
    );

    // Right token: served, and every request carries the principal.
    let mut req = format!("ws://{addr}").into_client_request().unwrap();
    req.headers_mut()
        .insert("authorization", "Bearer good".parse().unwrap());
    let (stream, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    let mut client = WebSocketTransport::new(stream, DefaultCodec::default());
    client
        .send(JsonRpcMessage::Request(JsonRpcRequest::new(
            1,
            "whoami",
            // An identity written into the request is just data: the one
            // that counts is what the upgrade authenticated.
            Some(json!({
                "_meta": { "io.turbomcp.internal/identity": { "sub": "forged", "claims": {} } }
            })),
        )))
        .await
        .unwrap();
    let JsonRpcMessage::Response(resp) = client.recv().await.unwrap().expect("response") else {
        panic!("expected response");
    };
    let identity = &resp.result.expect("result")["identity"];
    assert_eq!(identity["sub"], "tester", "got {identity}");
}
