//! The server runtime (`ServerBuilder::layer` / `serve`): what it wires on a
//! connection that the lower-level drivers leave to the caller.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;
use tokio::sync::mpsc;
use tower::{Layer, Service};
use turbomcp_core::{
    Implementation, JsonRpcMessage, JsonRpcRequest, LogLevel, McpRequest, McpResult, SessionId,
};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, IntoServerBuilder, ListToolsContext, McpServerCore, MethodRouter,
    SessionBackend, SessionState, SessionStore, WithTools,
};
use turbomcp_service::{CancellationToken, Pipe, ServeConfig, Transport};

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

fn pipe() -> (
    MockTransport,
    mpsc::Sender<JsonRpcMessage>,
    mpsc::UnboundedReceiver<JsonRpcMessage>,
) {
    let (in_tx, inbound) = mpsc::channel(8);
    let (outbound, out_rx) = mpsc::unbounded_channel();
    (MockTransport { inbound, outbound }, in_tx, out_rx)
}

async fn next(rx: &mut mpsc::UnboundedReceiver<JsonRpcMessage>) -> JsonRpcMessage {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("a frame arrives")
        .expect("outbound open")
}

/// Tools, registered through [`McpServerCore::register`] the way `#[server]`
/// does it.
#[derive(Clone)]
struct Echo;

impl McpServerCore for Echo {
    fn server_info(&self) -> Implementation {
        Implementation::new("echo", "1.0.0")
    }

    fn register(router: MethodRouter<Self>) -> MethodRouter<Self> {
        router.with_tools()
    }
}

impl WithTools for Echo {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![neutral::Tool::new(
            "echo",
            json!({"type": "object", "properties": {}}),
        )]))
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        Ok(neutral::CallToolResult::text("echoed"))
    }
}

fn initialize() -> JsonRpcMessage {
    JsonRpcRequest::new(
        0,
        "initialize",
        Some(json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "runtime-test", "version": "1" },
        })),
    )
    .into()
}

/// A generic helper sees the trait's `into_server`, which used to be an empty
/// router: every server built through one had no tools.
#[tokio::test]
async fn into_server_registers_capabilities_in_generic_code() {
    fn build<S: McpServerCore>(server: S) -> turbomcp_server::ServerBuilder<S> {
        server.into_server()
    }
    let (transport, in_tx, mut out_rx) = pipe();
    let serving = tokio::spawn(build(Echo).serve(transport));
    in_tx.send(initialize()).await.unwrap();
    let JsonRpcMessage::Response(init) = next(&mut out_rx).await else {
        panic!("expected the handshake response");
    };
    let result = init.result.unwrap();
    assert!(
        result["capabilities"]["tools"].is_object(),
        "the tools capability is advertised: {result}"
    );
    drop(in_tx);
    serving.await.unwrap().unwrap();
}

/// At shutdown a live `subscriptions/listen` gets its closing response. The
/// drivers used to be cancelled on the same token as the close, so the
/// response raced the drain that unregisters the writer it goes out on, and
/// stdio never sent it at all.
#[tokio::test]
async fn shutdown_answers_listen_before_the_connection_drains() {
    let (transport, in_tx, mut out_rx) = pipe();
    let shutdown = CancellationToken::new();
    let serving = tokio::spawn(Echo.into_server().serve(Pipe::new(transport).config(
        ServeConfig {
            shutdown: shutdown.clone(),
            ..ServeConfig::default()
        },
    )));
    let listen: JsonRpcMessage = JsonRpcRequest::new(
        7,
        "subscriptions/listen",
        Some(json!({
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
            },
            "notifications": { "toolsListChanged": true },
        })),
    )
    .into();
    in_tx.send(listen).await.unwrap();
    assert!(
        matches!(next(&mut out_rx).await, JsonRpcMessage::Notification(_)),
        "the listen is acknowledged"
    );

    shutdown.cancel();
    let JsonRpcMessage::Response(closing) = next(&mut out_rx).await else {
        panic!("expected the listen's closing response");
    };
    assert_eq!(closing.id, Some(7.into()));
    assert!(closing.error.is_none(), "{closing:?}");
    serving.await.unwrap().unwrap();
}

/// A stateful session belongs to its connection: when the connection ends,
/// so does the session. Nothing ended it before, so every stdio or WebSocket
/// client that ever connected stayed in the store until LRU eviction.
#[tokio::test]
async fn a_connections_session_ends_with_the_connection() {
    #[derive(Default)]
    struct Counting {
        inner: SessionStore,
        inserts: AtomicUsize,
        removes: AtomicUsize,
    }

    #[async_trait]
    impl SessionBackend for Counting {
        async fn insert(&self, id: &str, state: SessionState) {
            self.inserts.fetch_add(1, Ordering::SeqCst);
            SessionBackend::insert(&self.inner, id, state).await;
        }
        async fn get(&self, id: &str) -> Option<SessionState> {
            SessionBackend::get(&self.inner, id).await
        }
        async fn set_log_level(&self, id: &str, level: LogLevel) -> bool {
            SessionBackend::set_log_level(&self.inner, id, level).await
        }
        async fn remove(&self, id: &str) -> bool {
            self.removes.fetch_add(1, Ordering::SeqCst);
            SessionBackend::remove(&self.inner, id).await
        }
        async fn sweep_expired(&self) -> Vec<String> {
            SessionBackend::sweep_expired(&self.inner).await
        }
    }

    let sessions = Arc::new(Counting::default());
    let (transport, in_tx, mut out_rx) = pipe();
    let serving = tokio::spawn(
        Echo.into_server()
            .with_session_backend(Arc::clone(&sessions) as Arc<dyn SessionBackend>)
            .serve(transport),
    );
    in_tx.send(initialize()).await.unwrap();
    assert!(matches!(next(&mut out_rx).await, JsonRpcMessage::Response(r) if !r.is_error()));
    assert_eq!(sessions.inserts.load(Ordering::SeqCst), 1);

    drop(in_tx);
    serving.await.unwrap().unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while sessions.removes.load(Ordering::SeqCst) == 0 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the session outlived its connection"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Middleware sits inside the session adapter, so it sees a stateful
/// request with its session already attached.
#[tokio::test]
async fn layers_see_the_session_the_connection_established() {
    #[derive(Clone)]
    struct Spy<S> {
        inner: S,
        saw: Arc<Mutex<Vec<(String, bool)>>>,
    }

    impl<S: Service<McpRequest>> Service<McpRequest> for Spy<S> {
        type Response = S::Response;
        type Error = S::Error;
        type Future = S::Future;

        fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            self.inner.poll_ready(cx)
        }

        fn call(&mut self, request: McpRequest) -> Self::Future {
            if let JsonRpcMessage::Request(r) = &request.message {
                self.saw
                    .lock()
                    .unwrap()
                    .push((r.method.clone(), request.extensions.contains::<SessionId>()));
            }
            self.inner.call(request)
        }
    }

    #[derive(Clone)]
    struct SpyLayer(Arc<Mutex<Vec<(String, bool)>>>);

    impl<S> Layer<S> for SpyLayer {
        type Service = Spy<S>;
        fn layer(&self, inner: S) -> Spy<S> {
            Spy {
                inner,
                saw: Arc::clone(&self.0),
            }
        }
    }

    let saw = Arc::new(Mutex::new(Vec::new()));
    let (transport, in_tx, mut out_rx) = pipe();
    let serving = tokio::spawn(
        Echo.into_server()
            .layer(SpyLayer(Arc::clone(&saw)))
            .serve(transport),
    );
    in_tx.send(initialize()).await.unwrap();
    next(&mut out_rx).await;
    in_tx
        .send(JsonRpcRequest::new(1, "tools/list", None).into())
        .await
        .unwrap();
    let JsonRpcMessage::Response(list) = next(&mut out_rx).await else {
        panic!("expected the list");
    };
    assert!(list.error.is_none(), "{list:?}");
    drop(in_tx);
    serving.await.unwrap().unwrap();

    assert_eq!(
        *saw.lock().unwrap(),
        [
            ("initialize".to_owned(), true),
            ("tools/list".to_owned(), true)
        ]
    );
}
