//! Legacy POST-stream recovery must preserve per-request cursors and cancellation.
#![cfg(all(feature = "client", feature = "http"))]
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use turbomcp::client::{Client, ClientBuilder, ConnectMode, HttpClientTransport};

struct Mock {
    pending: Mutex<HashMap<String, (Value, Instant)>>,
    posts: AtomicUsize,
    resumes: AtomicUsize,
    retry_ms: u64,
    complete_in_post: bool,
}
async fn handle_post(State(s): State<Arc<Mock>>, Json(msg): Json<Value>) -> Response {
    let method = msg["method"].as_str().unwrap();
    if method == "notifications/initialized" {
        return StatusCode::ACCEPTED.into_response();
    }
    if method == "tools/list" {
        s.posts.fetch_add(1, Ordering::SeqCst);
        if s.complete_in_post {
            let reply = json!({"jsonrpc":"2.0","id":msg["id"],"result":{"tools":[]}});
            return (
                [("content-type", "text/event-stream")],
                format!("id: complete\nretry: {}\ndata: {reply}\n\n", s.retry_ms),
            )
                .into_response();
        }
        let cursor = format!("request-{}", msg["id"]);
        s.pending
            .lock()
            .unwrap()
            .insert(cursor.clone(), (msg["id"].clone(), Instant::now()));
        return (
            [("content-type", "text/event-stream")],
            format!("id: {cursor}\nretry: {}\ndata: \n\n", s.retry_ms),
        )
            .into_response();
    }
    assert_eq!(method, "initialize");
    ([("mcp-session-id","resume-session")],Json(json!({"jsonrpc":"2.0","id":msg["id"],"result":{"protocolVersion":"2025-11-25","serverInfo":{"name":"resume-test","version":"1"},"capabilities":{"tools":{}}}}))).into_response()
}
async fn handle_get(State(s): State<Arc<Mock>>, headers: HeaderMap) -> Response {
    let Some(cursor) = headers.get("last-event-id") else {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    };
    assert_eq!(headers["mcp-session-id"], "resume-session");
    assert_eq!(headers["mcp-protocol-version"], "2025-11-25");
    assert_eq!(headers["authorization"], "Bearer test-token");
    let cursor = cursor.to_str().unwrap();
    let (id, start) = s
        .pending
        .lock()
        .unwrap()
        .remove(cursor)
        .expect("per-request cursor");
    assert!(
        start.elapsed() >= Duration::from_millis(s.retry_ms),
        "reconnected before retry"
    );
    s.resumes.fetch_add(1, Ordering::SeqCst);
    let result = json!({"jsonrpc":"2.0","id":id,"result":{"tools":[{"name":cursor,"inputSchema":{"type":"object"}}]}});
    (
        [("content-type", "text/event-stream")],
        format!("id: final-{cursor}\ndata: {result}\n\n"),
    )
        .into_response()
}
struct Fixture {
    client: Client,
    state: Arc<Mock>,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}
async fn fixture(retry_ms: u64) -> Fixture {
    fixture_with_completion(retry_ms, false).await
}
async fn fixture_with_completion(retry_ms: u64, complete_in_post: bool) -> Fixture {
    let state = Arc::new(Mock {
        pending: Mutex::new(HashMap::new()),
        posts: AtomicUsize::new(0),
        resumes: AtomicUsize::new(0),
        retry_ms,
        complete_in_post,
    });
    let app = Router::new()
        .route(
            "/mcp",
            post(handle_post)
                .get(handle_get)
                .delete(|| async { StatusCode::NO_CONTENT }),
        )
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let transport = HttpClientTransport::new(url)
        .unwrap()
        .with_bearer("test-token");
    let client = ClientBuilder::new("resume-client", "1")
        .with_connect_mode(ConnectMode::Legacy)
        .with_timeout(Duration::from_secs(3))
        .connect(transport)
        .await
        .unwrap();
    Fixture {
        client,
        state,
        server,
    }
}
#[tokio::test]
async fn concurrent_post_streams_resume_with_separate_cursors_without_reposting() {
    let f = fixture(80).await;
    let (a, b) = tokio::join!(f.client.list_tools(None), f.client.list_tools(None));
    let a = a.unwrap();
    let b = b.unwrap();
    assert_ne!(a.tools[0].name, b.tools[0].name);
    assert_eq!(f.state.posts.load(Ordering::SeqCst), 2);
    assert_eq!(f.state.resumes.load(Ordering::SeqCst), 2);
    f.client.connection().close().await;
}
#[tokio::test]
async fn cancelling_during_retry_prevents_get_and_close_waits_for_cleanup() {
    let f = fixture(300).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(60), f.client.list_tools(None))
            .await
            .is_err()
    );
    assert_eq!(f.state.posts.load(Ordering::SeqCst), 1);
    tokio::time::timeout(Duration::from_secs(1), f.client.connection().close())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(f.state.resumes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn excessive_retry_is_refused_instead_of_reconnecting_too_early() {
    let f = fixture(30_001).await;
    let result = tokio::time::timeout(Duration::from_secs(1), f.client.list_tools(None))
        .await
        .expect("oversized retry fails promptly");
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("SSE retry delay exceeds recovery limit")
    );
    assert_eq!(f.state.resumes.load(Ordering::SeqCst), 0);
    f.client.connection().close().await;
}

#[tokio::test]
async fn retry_limit_does_not_discard_a_completed_response() {
    let f = fixture_with_completion(30_001, true).await;
    assert!(f.client.list_tools(None).await.unwrap().tools.is_empty());
    assert_eq!(f.state.resumes.load(Ordering::SeqCst), 0);
    f.client.connection().close().await;
}
