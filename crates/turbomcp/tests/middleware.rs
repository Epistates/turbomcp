//! RPC middleware: user-written `tower::Layer`s over the dispatcher.
//!
//! The seam these tests guard is the reason v4 dropped v3's `McpMiddleware`
//! trait. v3 had one hook per operation, so a middleware written before a method
//! existed could never observe it, and every protocol addition meant a new trait
//! method. A `Layer` over `Service<JsonRpcMessage>` has no such list — which is
//! only true if the frame seam really is universal, so that is what
//! `a_layer_observes_every_method` asserts, method by method.
//!
//! The other two cover the parts a layer author can get wrong: refusing a
//! request without the dispatcher running, and holding state across the clone
//! the driver makes for every inbound frame.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use serde_json::{Value, json};
use tokio::io::BufReader;
use turbomcp::methods::request;
use turbomcp::prelude::*;
use turbomcp::tower::{Layer, Service, ServiceBuilder, ServiceExt};
use turbomcp::{
    DefaultCodec, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, LegacySessionAdapter,
    ProtocolError, VersionDispatcher, mcp_to_jsonrpc_error, serve,
};
use turbomcp_transport_stdio::LineTransport;

// ---- a server covering every dispatched method -------------------------------

#[derive(Clone, Default)]
struct Everything {
    /// Bumped by `guarded`, so a test can prove a refusal never reached it.
    guarded_calls: Arc<AtomicUsize>,
}

#[server(name = "everything", version = "1.0.0")]
impl Everything {
    #[tool]
    async fn plain(&self) -> String {
        "ok".into()
    }

    /// The tool `RefuseLayer` denies below.
    #[tool]
    async fn guarded(&self) -> String {
        self.guarded_calls.fetch_add(1, Ordering::SeqCst);
        "ran".into()
    }

    #[resource("config://app")]
    async fn config(&self) -> McpResult<String> {
        Ok("{}".into())
    }

    #[prompt]
    async fn summarize(&self, text: String) -> String {
        text
    }

    #[completion]
    async fn complete(&self, _p: neutral::CompleteParams) -> McpResult<neutral::CompleteResult> {
        Ok(neutral::CompleteResult::new(vec![]))
    }
}

// ---- a layer that records what it saw ----------------------------------------

#[derive(Clone, Default)]
struct RecordLayer {
    seen: Arc<Mutex<Vec<String>>>,
}

impl RecordLayer {
    fn seen(&self) -> Vec<String> {
        self.seen.lock().expect("poisoned").clone()
    }
}

impl<S> Layer<S> for RecordLayer {
    type Service = Record<S>;

    fn layer(&self, inner: S) -> Self::Service {
        Record {
            inner,
            seen: Arc::clone(&self.seen),
        }
    }
}

#[derive(Clone)]
struct Record<S> {
    inner: S,
    seen: Arc<Mutex<Vec<String>>>,
}

impl<S> Service<JsonRpcMessage> for Record<S>
where
    S: Service<JsonRpcMessage, Response = Option<JsonRpcMessage>, Error = ProtocolError>,
    S::Future: Send + 'static,
{
    type Response = Option<JsonRpcMessage>;
    type Error = ProtocolError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: JsonRpcMessage) -> Self::Future {
        self.seen
            .lock()
            .expect("poisoned")
            .push(req.method().unwrap_or("(response)").to_owned());
        Box::pin(self.inner.call(req))
    }
}

// ---- a layer that refuses before the inner service runs ----------------------

#[derive(Clone)]
struct RefuseLayer(&'static str);

impl<S> Layer<S> for RefuseLayer {
    type Service = Refuse<S>;

    fn layer(&self, inner: S) -> Self::Service {
        Refuse {
            inner,
            tool: self.0,
        }
    }
}

#[derive(Clone)]
struct Refuse<S> {
    inner: S,
    tool: &'static str,
}

impl<S> Service<JsonRpcMessage> for Refuse<S>
where
    S: Service<JsonRpcMessage, Response = Option<JsonRpcMessage>, Error = ProtocolError>,
    S::Future: Send + 'static,
{
    type Response = Option<JsonRpcMessage>;
    type Error = ProtocolError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: JsonRpcMessage) -> Self::Future {
        if let JsonRpcMessage::Request(r) = &req
            && r.method == request::TOOLS_CALL
            && r.params
                .as_ref()
                .and_then(|p| p.get("name"))
                .and_then(Value::as_str)
                == Some(self.tool)
        {
            let error =
                mcp_to_jsonrpc_error(&McpError::permission_denied("not in this deployment"));
            let response = JsonRpcResponse::error(r.id.clone(), error);
            return Box::pin(std::future::ready(Ok(Some(response.into()))));
        }
        Box::pin(self.inner.call(req))
    }
}

// ---- harness -----------------------------------------------------------------

type Stack = Record<Refuse<LegacySessionAdapter<VersionDispatcher<Everything>>>>;

fn stack(record: RecordLayer, server: Everything) -> Stack {
    ServiceBuilder::new()
        .layer(record)
        .layer(RefuseLayer("guarded"))
        .service(LegacySessionAdapter::new(server.into_server().build()))
}

async fn call(svc: &Stack, id: i64, method: &str, params: Value) -> JsonRpcResponse {
    let req = JsonRpcRequest::new(id, method, Some(params));
    match svc
        .clone()
        .oneshot(req.into())
        .await
        .expect("service failed")
    {
        Some(JsonRpcMessage::Response(r)) => r,
        other => panic!("expected a response for {method}, got {other:?}"),
    }
}

fn initialize_params() -> Value {
    json!({
        "protocolVersion": "2025-11-25",
        "capabilities": {},
        "clientInfo": { "name": "t", "version": "1" },
    })
}

/// Every dispatched method reaches a layer. v3's per-operation hook list is the
/// thing this replaces, so a gap here would mean the replacement is worse.
#[tokio::test]
async fn a_layer_observes_every_method() {
    let record = RecordLayer::default();
    let svc = stack(record.clone(), Everything::default());

    let init = call(&svc, 1, request::INITIALIZE, initialize_params()).await;
    assert!(init.error.is_none(), "handshake failed: {:?}", init.error);

    let methods = [
        (request::TOOLS_LIST, json!({})),
        (
            request::TOOLS_CALL,
            json!({ "name": "plain", "arguments": {} }),
        ),
        (request::RESOURCES_LIST, json!({})),
        (request::RESOURCES_TEMPLATES_LIST, json!({})),
        (request::RESOURCES_READ, json!({ "uri": "config://app" })),
        (request::PROMPTS_LIST, json!({})),
        (
            request::PROMPTS_GET,
            json!({ "name": "summarize", "arguments": { "text": "hi" } }),
        ),
        (
            request::COMPLETION_COMPLETE,
            json!({
                "ref": { "type": "ref/prompt", "name": "summarize" },
                "argument": { "name": "text", "value": "h" },
            }),
        ),
        (request::PING, json!({})),
    ];

    for (i, (method, params)) in methods.iter().enumerate() {
        let id = 2 + i as i64;
        let resp = call(&svc, id, method, params.clone()).await;
        assert!(resp.error.is_none(), "{method} failed: {:?}", resp.error);
    }

    let mut expected = vec![request::INITIALIZE.to_string()];
    expected.extend(methods.iter().map(|(m, _)| (*m).to_string()));
    assert_eq!(
        record.seen(),
        expected,
        "the layer must see every method, in order"
    );
}

/// Refusing means the dispatcher never runs — the guarded tool's side effect
/// must not fire — and the code is the SDK's own, not a hand-picked number.
#[tokio::test]
async fn a_layer_refuses_before_the_dispatcher_runs() {
    let server = Everything::default();
    let guarded_calls = Arc::clone(&server.guarded_calls);
    let record = RecordLayer::default();
    let svc = stack(record, server);

    call(&svc, 1, request::INITIALIZE, initialize_params()).await;

    let allowed = call(
        &svc,
        2,
        request::TOOLS_CALL,
        json!({ "name": "plain", "arguments": {} }),
    )
    .await;
    assert!(allowed.error.is_none());

    let refused = call(
        &svc,
        3,
        request::TOOLS_CALL,
        json!({ "name": "guarded", "arguments": {} }),
    )
    .await;
    let error = refused
        .error
        .expect("the refusal must be an error response");
    assert_eq!(
        error.code,
        McpError::permission_denied("x").jsonrpc_code(),
        "a refusal must carry the SDK's canonical code"
    );
    assert_eq!(
        guarded_calls.load(Ordering::SeqCst),
        0,
        "the handler ran despite the refusal"
    );
}

/// The `serve` driver clones the whole stack for each inbound frame. A layer
/// holding state behind an `Arc` must therefore see all of them — this drives
/// real frames through the real driver rather than calling the service directly,
/// because the cloning is the driver's behavior, not the service's.
#[tokio::test]
async fn layer_state_survives_the_drivers_per_request_clone() {
    let record = RecordLayer::default();
    let svc = stack(record.clone(), Everything::default());

    let frames = [
        JsonRpcRequest::new(1, request::INITIALIZE, Some(initialize_params())),
        JsonRpcRequest::new(2, request::TOOLS_LIST, Some(json!({}))),
        JsonRpcRequest::new(3, request::PING, Some(json!({}))),
    ]
    .iter()
    .map(|r| serde_json::to_string(r).expect("serialize"))
    .collect::<Vec<_>>()
    .join("\n")
        + "\n";

    let transport = LineTransport::new(
        BufReader::new(std::io::Cursor::new(frames.into_bytes())),
        Vec::new(),
        DefaultCodec::default(),
    );
    serve(transport, svc).await.expect("driver failed");

    assert_eq!(
        record.seen(),
        vec![
            request::INITIALIZE.to_string(),
            request::TOOLS_LIST.to_string(),
            request::PING.to_string(),
        ],
        "one clone per frame must still accumulate into the shared state"
    );
}
