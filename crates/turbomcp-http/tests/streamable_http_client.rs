//! Wire-level contract tests for the Streamable HTTP client transport.
//!
//! Each test stands up a small scripted server and watches what the client
//! actually puts on the wire, so a regression shows up the way a real server
//! would see it.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use futures::StreamExt;
use turbomcp_http::{
    RetryPolicy, StreamableHttpClientConfig, StreamableHttpClientTransport, Transport,
    TransportError, TransportMessage,
};
use turbomcp_protocol::MessageId;

/// One request as the server saw it.
#[derive(Debug, Clone)]
struct Seen {
    method: Method,
    headers: HeaderMap,
    at: Instant,
}

impl Seen {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }
}

#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<Seen>>>);

impl Log {
    fn record(&self, method: &Method, headers: &HeaderMap) -> usize {
        let mut seen = self.0.lock().unwrap();
        seen.push(Seen {
            method: method.clone(),
            headers: headers.clone(),
            at: Instant::now(),
        });
        seen.len()
    }

    fn all(&self, method: Method) -> Vec<Seen> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|seen| seen.method == method)
            .cloned()
            .collect()
    }

    /// Wait for the `n`th request of `method` to arrive.
    async fn nth(&self, method: Method, n: usize) -> Seen {
        for _ in 0..100 {
            if let Some(seen) = self.all(method.clone()).get(n - 1) {
                return seen.clone();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("request {n} of {method} never arrived");
    }
}

/// What the scripted server answers: given the method, the headers, and how
/// many requests it has seen so far (this one included).
type Script = Arc<dyn Fn(&Method, &HeaderMap, usize) -> Response + Send + Sync>;

async fn serve(log: Log, script: Script) -> String {
    let app = Router::new().route(
        "/mcp",
        any(move |method: Method, headers: HeaderMap| {
            let log = log.clone();
            let script = Arc::clone(&script);
            async move {
                let n = log.record(&method, &headers);
                script(&method, &headers, n)
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

/// An SSE body: each chunk after its delay, then the end of the stream.
fn sse(chunks: Vec<(u64, &'static str)>) -> Response {
    let body = futures::stream::iter(chunks).then(|(delay, chunk)| async move {
        tokio::time::sleep(Duration::from_millis(delay)).await;
        Ok::<_, Infallible>(Bytes::from(chunk))
    });
    (
        [(header::CONTENT_TYPE, "text/event-stream")],
        Body::from_stream(body),
    )
        .into_response()
}

/// An SSE body that stays open after its chunks.
fn sse_then_hold(chunks: Vec<&'static str>) -> Response {
    let body = futures::stream::iter(
        chunks
            .into_iter()
            .map(|c| Ok::<_, Infallible>(Bytes::from(c))),
    )
    .chain(futures::stream::pending());
    (
        [(header::CONTENT_TYPE, "text/event-stream")],
        Body::from_stream(body),
    )
        .into_response()
}

fn initialize_result(version: &str, session_id: &str) -> Response {
    (
        [
            (header::CONTENT_TYPE, "application/json"),
            (
                header::HeaderName::from_static("mcp-session-id"),
                session_id,
            ),
        ],
        format!(r#"{{"jsonrpc":"2.0","id":0,"result":{{"protocolVersion":"{version}"}}}}"#),
    )
        .into_response()
}

fn client(base_url: &str, config: StreamableHttpClientConfig) -> StreamableHttpClientTransport {
    StreamableHttpClientTransport::new(StreamableHttpClientConfig {
        base_url: base_url.to_string(),
        endpoint_path: "/mcp".to_string(),
        ..config
    })
    .unwrap()
}

fn request(id: u64, method: &str) -> TransportMessage {
    TransportMessage::new(
        MessageId::from(id.to_string()),
        Bytes::from(format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}"}}"#
        )),
    )
}

async fn next_message(transport: &StreamableHttpClientTransport) -> serde_json::Value {
    let message = tokio::time::timeout(Duration::from_secs(3), transport.recv_async())
        .await
        .expect("a message should have been queued")
        .unwrap();
    serde_json::from_slice(&message.payload).unwrap()
}

/// `timeout` used to be reqwest's whole-request timeout, which runs until the
/// body is fully read — so it cut off any POST stream outliving it. The call
/// then returned `Ok` with no response queued, and the caller waited forever.
#[tokio::test]
async fn a_post_stream_may_outlive_the_request_timeout() {
    let log = Log::default();
    let base_url = serve(
        log.clone(),
        Arc::new(|_, _, _| {
            sse(vec![
                (0, "id: p-0\ndata:\n\n"),
                (
                    700,
                    "id: p-1\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n\n",
                ),
            ])
        }),
    )
    .await;
    let transport = client(
        &base_url,
        StreamableHttpClientConfig {
            timeout: Duration::from_millis(300),
            ..Default::default()
        },
    );

    transport.send(request(1, "tools/call")).await.unwrap();
    assert_eq!(next_message(&transport).await["id"], 1);
}

/// §Resumability: a stream cut before its response is resumed by a GET naming
/// *that stream's* last event id, after the server's `retry`. The client used
/// to keep one cursor for every stream — sending it on POSTs, which may not
/// carry it — and gave up on a cut POST stream silently, reporting success.
#[tokio::test]
async fn a_cut_post_stream_resumes_from_its_own_last_event_id() {
    let log = Log::default();
    let base_url = serve(
        log.clone(),
        Arc::new(|method, _, n| match *method {
            // A stream that runs to completion, leaving event ids behind.
            Method::POST if n == 1 => sse(vec![(
                0,
                "id: first-0\ndata:\n\nid: first-1\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n\n",
            )]),
            // Primed, then the connection ends with the call still running.
            Method::POST => sse(vec![(0, "retry: 300\nid: cut-0\ndata:\n\n")]),
            Method::GET => sse(vec![(
                0,
                "id: cut-1\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{}}\n\n",
            )]),
            _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
        }),
    )
    .await;
    let transport = client(&base_url, StreamableHttpClientConfig::default());

    transport.send(request(1, "tools/list")).await.unwrap();
    assert_eq!(next_message(&transport).await["id"], 1);

    transport.send(request(2, "tools/call")).await.unwrap();
    assert_eq!(
        next_message(&transport).await["id"],
        2,
        "the response is collected from the resumed stream"
    );

    let posts = log.all(Method::POST);
    assert!(
        posts
            .iter()
            .all(|post| post.header("last-event-id").is_none()),
        "a POST never carries Last-Event-ID"
    );
    let resume = log.nth(Method::GET, 1).await;
    assert_eq!(resume.header("last-event-id"), Some("cut-0"));
    assert!(
        resume.at.duration_since(posts[1].at) >= Duration::from_millis(300),
        "the server's retry is respected before reconnecting"
    );
}

/// A stream that ends before its response and never carried an event id
/// cannot be resumed. That is an error, not a silent success.
#[tokio::test]
async fn a_post_stream_that_cannot_be_resumed_is_an_error() {
    let log = Log::default();
    let base_url = serve(
        log.clone(),
        Arc::new(|_, _, _| sse(vec![(0, ": nothing to see\n\n")])),
    )
    .await;
    let transport = client(&base_url, StreamableHttpClientConfig::default());

    let result = transport.send(request(1, "tools/call")).await;
    assert!(
        matches!(result, Err(TransportError::ConnectionLost(_))),
        "got {result:?}"
    );
}

/// The standalone GET and the closing DELETE carry the same headers as every
/// POST: the negotiated protocol version, credentials, custom headers. The GET
/// used to send the configured version — a 400 from any server that had
/// negotiated down — and no custom headers; the DELETE sent none of them.
#[tokio::test]
async fn get_and_delete_carry_the_negotiated_version_and_every_header() {
    let log = Log::default();
    let base_url = serve(
        log.clone(),
        Arc::new(|method, _, _| match *method {
            Method::POST => initialize_result("2025-06-18", "sess-1"),
            Method::GET => sse_then_hold(vec!["id: g-0\ndata:\n\n"]),
            _ => StatusCode::NO_CONTENT.into_response(),
        }),
    )
    .await;
    let transport = client(
        &base_url,
        StreamableHttpClientConfig {
            protocol_version: "2025-11-25".to_string(),
            auth_token: Some("tok".to_string()),
            headers: HashMap::from([("x-tenant".to_string(), "acme".to_string())]),
            ..Default::default()
        },
    );

    transport.send(request(0, "initialize")).await.unwrap();
    let get = log.nth(Method::GET, 1).await;
    transport.disconnect().await.unwrap();
    let delete = log.nth(Method::DELETE, 1).await;

    for (seen, what) in [(&get, "GET"), (&delete, "DELETE")] {
        assert_eq!(
            seen.header("mcp-protocol-version"),
            Some("2025-06-18"),
            "{what}"
        );
        assert_eq!(seen.header("mcp-session-id"), Some("sess-1"), "{what}");
        assert_eq!(seen.header("authorization"), Some("Bearer tok"), "{what}");
        assert_eq!(seen.header("x-tenant"), Some("acme"), "{what}");
        assert_eq!(seen.header("last-event-id"), None, "{what}");
    }
}

/// §Session Management: a 404 for a request carrying the session id means the
/// session is gone, and the client "MUST start a new session". The transport
/// reports that as its own error, and forgets the session and the version
/// negotiated for it so the next `initialize` really does start afresh.
#[tokio::test]
async fn a_404_for_the_session_is_a_typed_expiry_that_forgets_it() {
    let log = Log::default();
    let base_url = serve(
        log.clone(),
        Arc::new(|method, headers, _| match *method {
            Method::POST if headers.contains_key("mcp-session-id") => {
                StatusCode::NOT_FOUND.into_response()
            }
            Method::POST => initialize_result("2025-06-18", "sess-1"),
            _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
        }),
    )
    .await;
    let transport = client(&base_url, StreamableHttpClientConfig::default());

    transport.send(request(0, "initialize")).await.unwrap();
    let result = transport.send(request(1, "tools/list")).await;
    assert!(
        matches!(result, Err(TransportError::SessionExpired(_))),
        "got {result:?}"
    );

    transport.send(request(2, "initialize")).await.unwrap();
    let reinitialize = log.nth(Method::POST, 3).await;
    assert_eq!(reinitialize.header("mcp-session-id"), None);
    assert_eq!(
        reinitialize.header("mcp-protocol-version"),
        Some("2025-11-25"),
        "the old session's negotiated version goes with it"
    );
}

/// §Listening for Messages: a server closing the GET stream SHOULD send
/// `retry`, and the client MUST wait that long before reconnecting — with the
/// stream's own `Last-Event-ID`. The field used to be ignored outright.
#[tokio::test]
async fn standalone_reconnects_honour_the_servers_retry() {
    let log = Log::default();
    let base_url = serve(
        log.clone(),
        Arc::new(|method, _, _| match *method {
            Method::POST => initialize_result("2025-11-25", "sess-1"),
            Method::GET => sse(vec![(0, "retry: 400\nid: g-0\ndata:\n\n")]),
            _ => StatusCode::NO_CONTENT.into_response(),
        }),
    )
    .await;
    let transport = client(
        &base_url,
        StreamableHttpClientConfig {
            // Every stream counts as healthy, so backoff alone would reconnect
            // at once: any wait is the server's.
            sse_healthy_stream_threshold: Duration::ZERO,
            retry_policy: RetryPolicy::Fixed {
                interval: Duration::from_millis(10),
                max_attempts: None,
            },
            ..Default::default()
        },
    );

    transport.send(request(0, "initialize")).await.unwrap();
    let first = log.nth(Method::GET, 1).await;
    let second = log.nth(Method::GET, 2).await;
    transport.disconnect().await.unwrap();

    assert!(
        second.at.duration_since(first.at) >= Duration::from_millis(380),
        "reconnected after {:?}",
        second.at.duration_since(first.at)
    );
    assert_eq!(second.header("last-event-id"), Some("g-0"));
}

/// An `endpoint` event redirects every later POST, bearer token included. One
/// naming another origin used to be followed, handing the credentials to
/// whoever could write to the stream.
#[tokio::test]
async fn an_endpoint_event_cannot_send_posts_to_another_origin() {
    let log = Log::default();
    let base_url = serve(
        log.clone(),
        Arc::new(|method, _, n| match *method {
            Method::POST if n == 1 => initialize_result("2025-11-25", "sess-1"),
            Method::POST => (
                [(header::CONTENT_TYPE, "application/json")],
                r#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
            )
                .into_response(),
            Method::GET => {
                sse_then_hold(vec!["event: endpoint\ndata: http://evil.invalid/steal\n\n"])
            }
            _ => StatusCode::NO_CONTENT.into_response(),
        }),
    )
    .await;
    let transport = client(
        &base_url,
        StreamableHttpClientConfig {
            auth_token: Some("secret".to_string()),
            ..Default::default()
        },
    );

    transport.send(request(0, "initialize")).await.unwrap();
    log.nth(Method::GET, 1).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    transport.send(request(1, "tools/list")).await.unwrap();
    let post = log.nth(Method::POST, 2).await;
    assert_eq!(post.header("authorization"), Some("Bearer secret"));
}

/// Hands out `fresh` once the server has challenged, and records what it
/// was told.
#[derive(Debug, Default)]
struct Reauthenticates {
    challenged: Mutex<Vec<turbomcp_http::AuthChallenge>>,
}

impl turbomcp_http::AuthProvider for Reauthenticates {
    fn token(&self) -> turbomcp_http::AuthFuture<'_, Option<String>> {
        let token = if self.challenged.lock().unwrap().is_empty() {
            "stale"
        } else {
            "fresh"
        };
        Box::pin(async move { Some(token.to_string()) })
    }

    fn on_challenge<'a>(
        &'a self,
        challenge: &'a turbomcp_http::AuthChallenge,
    ) -> turbomcp_http::AuthFuture<'a, bool> {
        self.challenged.lock().unwrap().push(challenge.clone());
        Box::pin(async { true })
    }
}

fn refuse_unless_fresh(headers: &HeaderMap) -> Option<Response> {
    let fresh = headers
        .get(header::AUTHORIZATION)
        .is_some_and(|value| value == "Bearer fresh");
    (!fresh).then(|| {
        (
            StatusCode::UNAUTHORIZED,
            [(
                header::WWW_AUTHENTICATE,
                r#"Bearer error="invalid_token", resource_metadata="https://mcp.example.com/.well-known/oauth-protected-resource/mcp""#,
            )],
        )
            .into_response()
    })
}

/// MCP authorization: a client MUST parse a 401's `WWW-Authenticate` and use
/// its `resource_metadata` to find where to get a token. The provider gets
/// the parsed challenge, and the request is retried once with what it
/// obtained.
#[tokio::test]
async fn a_challenge_reaches_the_auth_provider_and_the_request_is_retried() {
    let log = Log::default();
    let base_url = serve(
        log.clone(),
        Arc::new(|method, headers, _| {
            if *method != Method::POST {
                return StatusCode::METHOD_NOT_ALLOWED.into_response();
            }
            refuse_unless_fresh(headers).unwrap_or_else(|| initialize_result("2025-11-25", "s-1"))
        }),
    )
    .await;

    let provider = Arc::new(Reauthenticates::default());
    let transport = client(
        &base_url,
        StreamableHttpClientConfig {
            auth_provider: Some(provider.clone()),
            ..Default::default()
        },
    );
    transport.send(request(0, "initialize")).await.unwrap();
    assert_eq!(next_message(&transport).await["id"], 0);

    let challenged = provider.challenged.lock().unwrap().clone();
    assert_eq!(challenged.len(), 1);
    assert_eq!(
        challenged[0].resource_metadata.as_deref(),
        Some("https://mcp.example.com/.well-known/oauth-protected-resource/mcp")
    );
    let posts = log.all(Method::POST);
    assert_eq!(posts[0].header("authorization"), Some("Bearer stale"));
    assert_eq!(posts[1].header("authorization"), Some("Bearer fresh"));
}

/// Without a provider the refusal is an authentication error that carries
/// the challenge, not a generic connection failure.
#[tokio::test]
async fn a_challenge_without_a_provider_is_an_authentication_error() {
    let base_url = serve(
        Log::default(),
        Arc::new(|_, headers, _| refuse_unless_fresh(headers).unwrap()),
    )
    .await;

    let transport = client(&base_url, StreamableHttpClientConfig::default());
    let error = transport
        .send(request(0, "initialize"))
        .await
        .expect_err("the server refused");
    match error {
        TransportError::AuthenticationFailed(detail) => {
            assert!(detail.contains("resource metadata at"), "{detail}")
        }
        other => panic!("expected an authentication failure, got {other:?}"),
    }
}
