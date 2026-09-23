//! What the client puts on the wire, and what it does with what comes back.
//!
//! Each test drives a real [`Client`] over an in-memory transport whose server
//! side is a script: the test decides how every request is answered and can
//! push server-initiated messages at any point.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc;
use turbomcp_client::handlers::{
    ElicitationCompleteHandler, ElicitationHandler, ElicitationRequest, ElicitationResponse,
    HandlerError, HandlerResult, ProgressHandler, ProgressNotification,
};
use turbomcp_client::{CallToolResponse, Client};
use turbomcp_protocol::MessageId;
use turbomcp_protocol::types::TaskMetadata;
use turbomcp_transport::{
    Transport, TransportCapabilities, TransportConfig, TransportError, TransportMessage,
    TransportMetrics, TransportResult, TransportState, TransportType,
};

type Responder = dyn Fn(&Value, &Server) -> Option<Value> + Send + Sync;

/// The server end of the in-memory wire.
#[derive(Clone)]
struct Server {
    sent: Arc<Mutex<Vec<Value>>>,
    inject: mpsc::UnboundedSender<Value>,
    expired: Arc<AtomicBool>,
}

impl Server {
    /// Push a server-initiated message to the client.
    fn send(&self, message: Value) {
        self.inject.send(message).expect("client transport alive");
    }

    /// Answer request `request` with `result`, at any time — for replies a
    /// test wants to send late, or after notifications of its own.
    fn reply(&self, request: &Value, result: Value) {
        self.send(json!({ "jsonrpc": "2.0", "id": request["id"], "result": result }));
    }

    /// Forget the session, as a Streamable HTTP server does: the next request
    /// other than `initialize` fails with `TransportError::SessionExpired`.
    fn expire_session(&self) {
        self.expired.store(true, Ordering::SeqCst);
    }

    fn received(&self) -> Vec<Value> {
        self.sent.lock().expect("sent log poisoned").clone()
    }

    fn received_method(&self, method: &str) -> Vec<Value> {
        self.received()
            .into_iter()
            .filter(|m| m["method"] == method)
            .collect()
    }

    /// Wait until the client has sent a response to server request `id`.
    async fn response_to(&self, id: &str) -> Value {
        for _ in 0..200 {
            if let Some(response) = self
                .received()
                .into_iter()
                .find(|m| m.get("method").is_none() && m["id"] == id)
            {
                return response;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("client never answered server request {id}");
    }
}

struct Wire {
    capabilities: TransportCapabilities,
    server: Server,
    respond: Box<Responder>,
    incoming: tokio::sync::Mutex<mpsc::UnboundedReceiver<Value>>,
}

impl std::fmt::Debug for Wire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wire").finish_non_exhaustive()
    }
}

/// A wire whose server answers `initialize` with `capabilities` and every
/// other request through `respond` (`None` leaves the request unanswered).
fn wire(
    capabilities: Value,
    respond: impl Fn(&Value) -> Option<Value> + Send + Sync + 'static,
) -> (Wire, Server) {
    scripted_wire(capabilities, move |request, _| respond(request))
}

/// [`wire`], with a responder that can also talk to the client through the
/// [`Server`] — to send notifications before its answer, or answer later.
fn scripted_wire(
    capabilities: Value,
    respond: impl Fn(&Value, &Server) -> Option<Value> + Send + Sync + 'static,
) -> (Wire, Server) {
    let (inject, incoming) = mpsc::unbounded_channel();
    let server = Server {
        sent: Arc::new(Mutex::new(Vec::new())),
        inject,
        expired: Arc::new(AtomicBool::new(false)),
    };
    let respond = move |request: &Value, server: &Server| {
        if request["method"] == "initialize" {
            return Some(json!({
                "protocolVersion": turbomcp_protocol::PROTOCOL_VERSION,
                "capabilities": capabilities,
                "serverInfo": { "name": "scripted", "version": "1.0.0" }
            }));
        }
        respond(request, server)
    };
    (
        Wire {
            capabilities: TransportCapabilities::default(),
            server: server.clone(),
            respond: Box::new(respond),
            incoming: tokio::sync::Mutex::new(incoming),
        },
        server,
    )
}

impl Transport for Wire {
    fn transport_type(&self) -> TransportType {
        TransportType::Stdio
    }

    fn capabilities(&self) -> &TransportCapabilities {
        &self.capabilities
    }

    fn state(&self) -> Pin<Box<dyn Future<Output = TransportState> + Send + '_>> {
        Box::pin(async { TransportState::Connected })
    }

    fn connect(&self) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }

    fn disconnect(&self) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }

    fn send(
        &self,
        message: TransportMessage,
    ) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
        let message: Value = serde_json::from_slice(&message.payload).expect("client sends JSON");
        self.server
            .sent
            .lock()
            .expect("sent log poisoned")
            .push(message.clone());

        let is_request = message.get("method").is_some() && message.get("id").is_some();
        if is_request
            && message["method"] != "initialize"
            && self.server.expired.swap(false, Ordering::SeqCst)
        {
            return Box::pin(async {
                Err(TransportError::SessionExpired(
                    "the server no longer knows this session (HTTP 404)".to_string(),
                ))
            });
        }
        if is_request && let Some(reply) = (self.respond)(&message, &self.server) {
            let envelope = if reply.get("error").is_some() {
                json!({ "jsonrpc": "2.0", "id": message["id"], "error": reply["error"] })
            } else {
                json!({ "jsonrpc": "2.0", "id": message["id"], "result": reply })
            };
            let _ = self.server.inject.send(envelope);
        }
        Box::pin(async { Ok(()) })
    }

    fn receive(
        &self,
    ) -> Pin<Box<dyn Future<Output = TransportResult<Option<TransportMessage>>> + Send + '_>> {
        Box::pin(async move {
            let message = self.incoming.lock().await.recv().await;
            Ok(message.map(|m| {
                TransportMessage::new(
                    MessageId::from("incoming"),
                    serde_json::to_vec(&m).expect("serializes").into(),
                )
            }))
        })
    }

    fn metrics(&self) -> Pin<Box<dyn Future<Output = TransportMetrics> + Send + '_>> {
        Box::pin(async { TransportMetrics::default() })
    }
}

/// Records every completion it is handed.
#[derive(Debug, Default)]
struct Completions(Mutex<Vec<String>>);

impl Completions {
    fn seen(&self) -> Vec<String> {
        self.0.lock().expect("poisoned").clone()
    }
}

impl ElicitationCompleteHandler for Completions {
    fn handle_elicitation_complete(
        &self,
        elicitation_id: String,
    ) -> Pin<Box<dyn Future<Output = HandlerResult<()>> + Send + '_>> {
        self.0.lock().expect("poisoned").push(elicitation_id);
        Box::pin(async { Ok(()) })
    }
}

/// Accepts every elicitation without content, as a URL mode handler does once
/// the URL has been shown.
#[derive(Debug)]
struct Consent;

impl ElicitationHandler for Consent {
    fn handle_elicitation(
        &self,
        _request: ElicitationRequest,
    ) -> Pin<Box<dyn Future<Output = HandlerResult<ElicitationResponse>> + Send + '_>> {
        Box::pin(async { Ok(ElicitationResponse::accept_without_content()) })
    }
}

/// Stands in for a user who closed the prompt without choosing.
#[derive(Debug)]
struct Dismissed;

impl ElicitationHandler for Dismissed {
    fn handle_elicitation(
        &self,
        _request: ElicitationRequest,
    ) -> Pin<Box<dyn Future<Output = HandlerResult<ElicitationResponse>> + Send + '_>> {
        Box::pin(async { Err(HandlerError::UserCancelled) })
    }
}

/// Never answers, and records when it is started and when it is dropped —
/// which, for a future that never finishes, only an abort can do.
#[derive(Debug, Default)]
struct Hangs {
    started: AtomicBool,
    dropped: Arc<AtomicBool>,
}

struct SetOnDrop(Arc<AtomicBool>);

impl Drop for SetOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

impl ElicitationHandler for Hangs {
    fn handle_elicitation(
        &self,
        _request: ElicitationRequest,
    ) -> Pin<Box<dyn Future<Output = HandlerResult<ElicitationResponse>> + Send + '_>> {
        self.started.store(true, Ordering::SeqCst);
        let on_drop = SetOnDrop(Arc::clone(&self.dropped));
        Box::pin(async move {
            let _on_drop = on_drop;
            std::future::pending().await
        })
    }
}

/// Records the progress values it is handed. The first is slow, which is
/// what let later notifications overtake it when each ran in its own task.
#[derive(Debug, Default)]
struct ProgressLog(Mutex<Vec<f64>>);

impl ProgressLog {
    fn seen(&self) -> Vec<f64> {
        self.0.lock().expect("poisoned").clone()
    }
}

impl ProgressHandler for ProgressLog {
    fn handle_progress(
        &self,
        notification: ProgressNotification,
    ) -> Pin<Box<dyn Future<Output = HandlerResult<()>> + Send + '_>> {
        Box::pin(async move {
            if notification.progress == 1.0 {
                tokio::time::sleep(Duration::from_millis(30)).await;
            }
            self.0.lock().expect("poisoned").push(notification.progress);
            Ok(())
        })
    }
}

fn progress(token: &Value, progress: u32) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "notifications/progress",
        "params": { "progressToken": token, "progress": progress }
    })
}

/// Server capabilities that allow task-augmented `tools/call`.
fn with_tool_tasks() -> Value {
    json!({ "tools": {}, "tasks": { "cancel": {}, "requests": { "tools": { "call": {} } } } })
}

/// Wait until the client has sent a message with `method`, up to a second.
async fn wait_for(server: &Server, method: &str) -> Vec<Value> {
    for _ in 0..100 {
        let sent = server.received_method(method);
        if !sent.is_empty() {
            return sent;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Vec::new()
}

fn complete(elicitation_id: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "notifications/elicitation/complete",
        "params": { "elicitationId": elicitation_id }
    })
}

/// Give the dispatcher a moment to route what was just injected.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(50)).await;
}

/// JSON-RPC: `params`, when present, MUST be a structured value. The
/// TypeScript SDK's schema rejects `"params": null`, so a client that sent it
/// on `notifications/initialized` failed the handshake with every server built
/// on that SDK — 400 over HTTP, silently dropped over stdio.
#[tokio::test]
async fn notifications_without_params_omit_the_member() {
    let (transport, server) = wire(json!({}), |_| None);
    let client = Client::new(transport);
    client.initialize().await.expect("handshake");

    let initialized = server.received_method("notifications/initialized");
    assert_eq!(initialized.len(), 1);
    assert!(
        initialized[0].get("params").is_none(),
        "`params` must be absent, not null: {}",
        initialized[0]
    );
}

/// elicitation.mdx: clients "MUST ignore notifications referencing unknown or
/// already-completed IDs". The handler used to fire for any id at all.
#[tokio::test]
async fn a_completion_for_an_id_never_issued_is_ignored() {
    let (transport, server) = wire(json!({}), |_| None);
    let client = Client::new(transport);
    let completions = Arc::new(Completions::default());
    client.set_elicitation_complete_handler(completions.clone());
    client.initialize().await.expect("handshake");

    server.send(complete("made-up"));
    settle().await;

    assert!(completions.seen().is_empty(), "{:?}", completions.seen());
}

/// A URL mode request the client accepted registers its id, so exactly one
/// completion for it gets through — a replay is "already completed".
#[tokio::test]
async fn a_url_elicitation_completes_exactly_once() {
    let (transport, server) = wire(json!({}), |_| None);
    let client = Client::new(transport);
    let completions = Arc::new(Completions::default());
    client.set_elicitation_handler(Arc::new(Consent));
    client.enable_elicitation_url();
    client.set_elicitation_complete_handler(completions.clone());
    client.initialize().await.expect("handshake");

    server.send(json!({
        "jsonrpc": "2.0",
        "id": "s-1",
        "method": "elicitation/create",
        "params": {
            "mode": "url",
            "message": "Connect your account",
            "url": "https://example.com/connect",
            "elicitationId": "e-1"
        }
    }));
    let response = server.response_to("s-1").await;
    assert_eq!(response["result"]["action"], "accept", "{response}");

    server.send(complete("e-1"));
    server.send(complete("e-1"));
    settle().await;

    assert_eq!(completions.seen(), vec!["e-1".to_string()]);
}

/// Without `enable_elicitation_url` the client declares `elicitation: {}`,
/// which is form only; with it, the declaration names both modes.
#[tokio::test]
async fn url_mode_is_declared_only_when_enabled() {
    for enable in [false, true] {
        let (transport, server) = wire(json!({}), |_| None);
        let client = Client::new(transport);
        client.set_elicitation_handler(Arc::new(Consent));
        if enable {
            client.enable_elicitation_url();
        }
        client.initialize().await.expect("handshake");

        let initialize = &server.received_method("initialize")[0];
        let elicitation = &initialize["params"]["capabilities"]["elicitation"];
        if enable {
            assert!(elicitation.get("url").is_some(), "{elicitation}");
            assert!(elicitation.get("form").is_some(), "{elicitation}");
        } else {
            assert_eq!(elicitation, &json!({}));
        }
    }
}

/// A -32042 answering one of our requests is how a server asks for a URL
/// interaction without a separate `elicitation/create`. Its `data` is the
/// payload — the URLs — and its ids are ones a completion may reference.
#[tokio::test]
async fn a_url_elicitation_required_error_keeps_its_data() {
    let data = json!({
        "elicitations": [{
            "mode": "url",
            "message": "Authorize access to your files.",
            "url": "https://example.com/connect",
            "elicitationId": "e-42"
        }]
    });
    let error_data = data.clone();
    let (transport, server) = wire(json!({ "tools": {} }), move |request| {
        (request["method"] == "tools/call").then(|| {
            json!({ "error": {
                "code": -32042,
                "message": "This request requires more information.",
                "data": error_data
            }})
        })
    });
    let client = Client::new(transport);
    let completions = Arc::new(Completions::default());
    client.set_elicitation_complete_handler(completions.clone());
    client.initialize().await.expect("handshake");

    let error = client
        .call_tool("read_files", Some(HashMap::new()), None)
        .await
        .expect_err("the server answered -32042");
    assert_eq!(error.jsonrpc_error_code(), -32042);
    assert_eq!(error.data(), Some(&data));

    server.send(complete("e-42"));
    settle().await;
    assert_eq!(completions.seen(), vec!["e-42".to_string()]);
}

/// A dismissed elicitation is `action: cancel`, the outcome elicitation.mdx
/// defines for it — not a JSON-RPC error. It used to go out as -1 "User
/// rejected sampling request", naming a feature the server never used.
#[tokio::test]
async fn a_user_cancelled_elicitation_answers_action_cancel() {
    let (transport, server) = wire(json!({}), |_| None);
    let client = Client::new(transport);
    client.set_elicitation_handler(Arc::new(Dismissed));
    client.initialize().await.expect("handshake");

    server.send(json!({
        "jsonrpc": "2.0",
        "id": "s-1",
        "method": "elicitation/create",
        "params": {
            "mode": "form",
            "message": "Pick a colour",
            "requestedSchema": { "type": "object", "properties": {} }
        }
    }));
    let response = server.response_to("s-1").await;

    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["action"], "cancel", "{response}");
}

/// Notifications reach their handlers one at a time, in arrival order, and a
/// call's progress has all been handled by the time the call returns. Each
/// notification used to run in its own task, so a slow first one was
/// overtaken by the rest — and the result came back before it was seen.
#[tokio::test]
async fn progress_is_handled_in_order_before_the_call_returns() {
    let (transport, _server) = scripted_wire(json!({ "tools": {} }), |request, server| {
        (request["method"] == "tools/call").then(|| {
            let token = &request["params"]["_meta"]["progressToken"];
            for step in 1..=5 {
                server.send(progress(token, step));
            }
            json!({ "content": [] })
        })
    });
    let client = Client::new(transport);
    let log = Arc::new(ProgressLog::default());
    client.set_progress_handler(log.clone());
    client.initialize().await.expect("handshake");

    client
        .call_tool_response_with_progress("work", None, None, Some(MessageId::from("p-1")))
        .await
        .expect("the call succeeds");

    assert_eq!(log.seen(), vec![1.0, 2.0, 3.0, 4.0, 5.0]);
}

/// progress.mdx: notifications "MUST only reference tokens that were provided
/// in an active request". One for a token never issued, or for a call that
/// has finished, is not progress on anything the caller is waiting for.
#[tokio::test]
async fn progress_for_an_unknown_or_finished_token_is_ignored() {
    let (transport, server) = wire(json!({ "tools": {} }), |request| {
        (request["method"] == "tools/call").then(|| json!({ "content": [] }))
    });
    let client = Client::new(transport);
    let log = Arc::new(ProgressLog::default());
    client.set_progress_handler(log.clone());
    client.initialize().await.expect("handshake");
    client
        .call_tool_response_with_progress("work", None, None, Some(MessageId::from("p-1")))
        .await
        .expect("the call succeeds");

    server.send(progress(&json!("p-1"), 2));
    server.send(progress(&json!("never-issued"), 3));
    settle().await;

    assert!(log.seen().is_empty(), "{:?}", log.seen());
}

/// A progress token is `string | integer`, and one already in use by a call
/// in flight is refused: it is what ties a notification to its call.
#[tokio::test]
async fn a_progress_token_in_use_is_refused() {
    let (transport, _server) = wire(json!({ "tools": {} }), |_| None);
    let client = Client::new(transport);
    client.initialize().await.expect("handshake");

    let pending = client.clone();
    let first = tokio::spawn(async move {
        pending
            .call_tool_response_with_progress("slow", None, None, Some(MessageId::from(7)))
            .await
    });
    settle().await;

    let second = client
        .call_tool_response_with_progress("other", None, None, Some(MessageId::from(7)))
        .await;
    assert!(second.is_err(), "{second:?}");
    first.abort();
}

/// The `total` timeout used to fail the call without telling the server,
/// which went on working for a client that had stopped listening.
#[tokio::test]
async fn the_total_timeout_cancels_the_request() {
    let (transport, server) = wire(json!({}), |_| None);
    let mut config = TransportConfig::default();
    config.timeouts.request = None;
    config.timeouts.total = Some(Duration::from_millis(50));
    let client = Client::new_with_config(transport, config);
    client.initialize().await.expect("handshake");

    client.list_tools().await.expect_err("never answered");

    let listed = &server.received_method("tools/list")[0];
    let cancelled = wait_for(&server, "notifications/cancelled").await;
    assert_eq!(cancelled.len(), 1, "{cancelled:?}");
    assert_eq!(cancelled[0]["params"]["requestId"], listed["id"]);
}

/// Dropping the call's future — `select!`, a UI cancel, an aborted task — is
/// abandoning the request too, and the server hears about it.
#[tokio::test]
async fn a_dropped_call_cancels_the_request() {
    let (transport, server) = wire(json!({}), |_| None);
    let client = Client::new(transport);
    client.initialize().await.expect("handshake");

    let caller = client.clone();
    let call = tokio::spawn(async move { caller.list_tools().await });
    settle().await;
    call.abort();

    let listed = &server.received_method("tools/list")[0];
    let cancelled = wait_for(&server, "notifications/cancelled").await;
    assert_eq!(cancelled.len(), 1, "{cancelled:?}");
    assert_eq!(cancelled[0]["params"]["requestId"], listed["id"]);
}

/// `with_timeout` bounds the calls made through it, and a timed-out call is
/// cancelled like any other.
#[tokio::test]
async fn with_timeout_bounds_a_single_call() {
    let (transport, server) = wire(json!({}), |_| None);
    let client = Client::new(transport);
    client.initialize().await.expect("handshake");

    let result = tokio::time::timeout(
        Duration::from_secs(2),
        client.with_timeout(Duration::from_millis(50)).list_tools(),
    )
    .await
    .expect("the 50ms timeout applies, not the 60s default");
    assert!(result.is_err());
    assert_eq!(wait_for(&server, "notifications/cancelled").await.len(), 1);
}

/// lifecycle.mdx: a client MAY restart a request's timeout on progress for
/// it. A call that reports progress every 50ms for 400ms outlives a 200ms
/// timeout.
#[tokio::test]
async fn progress_restarts_the_request_timeout() {
    let (transport, _server) = scripted_wire(json!({ "tools": {} }), |request, server| {
        if request["method"] != "tools/call" {
            return None;
        }
        let (request, server) = (request.clone(), server.clone());
        tokio::spawn(async move {
            let token = &request["params"]["_meta"]["progressToken"];
            for step in 1..=8 {
                tokio::time::sleep(Duration::from_millis(50)).await;
                server.send(progress(token, step));
            }
            server.reply(&request, json!({ "content": [] }));
        });
        None
    });
    let client = Client::new(transport);
    client.initialize().await.expect("handshake");

    client
        .with_timeout(Duration::from_millis(200))
        .call_tool_response_with_progress("work", None, None, Some(MessageId::from("p-1")))
        .await
        .expect("progress kept the call alive past its 200ms timeout");
}

/// cancellation.mdx: a task-augmented request is cancelled with
/// `tasks/cancel`, never `notifications/cancelled`. The task id only exists
/// once the `CreateTaskResult` arrives, so an abandoned call waits for it.
#[tokio::test]
async fn an_abandoned_task_augmented_call_cancels_its_task() {
    let (transport, server) = scripted_wire(with_tool_tasks(), |request, server| {
        if request["method"] != "tools/call" {
            return None;
        }
        let (request, server) = (request.clone(), server.clone());
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            server.reply(
                &request,
                json!({ "task": {
                    "taskId": "t-1",
                    "status": "working",
                    "createdAt": "2025-11-25T10:30:00Z",
                    "lastUpdatedAt": "2025-11-25T10:30:00Z",
                    "ttl": null
                }}),
            );
        });
        None
    });
    let client = Client::new(transport);
    client.initialize().await.expect("handshake");

    client
        .with_timeout(Duration::from_millis(50))
        .call_tool_task("work", None, TaskMetadata { ttl: None })
        .await
        .expect_err("timed out before the task was created");

    let cancels = wait_for(&server, "tasks/cancel").await;
    assert_eq!(cancels.len(), 1, "{:?}", server.received());
    assert_eq!(cancels[0]["params"]["taskId"], "t-1");
    assert!(server.received_method("notifications/cancelled").is_empty());
}

/// tasks.mdx: without `tasks.requests.tools.call` a client "MUST NOT attempt
/// to use task augmentation". The call is refused before it is sent.
#[tokio::test]
async fn call_tool_task_requires_the_tasks_capability() {
    let (transport, server) = wire(json!({ "tools": {} }), |_| None);
    let client = Client::new(transport);
    client.initialize().await.expect("handshake");

    let result = client
        .call_tool_task("work", None, TaskMetadata { ttl: None })
        .await;

    assert!(result.is_err(), "{result:?}");
    assert!(server.received_method("tools/call").is_empty());
}

/// A server may run a task-augmented call inline and answer with a plain
/// result. The tool has run; its result used to be discarded for an error.
#[tokio::test]
async fn call_tool_task_keeps_a_plain_result() {
    let (transport, _server) = wire(with_tool_tasks(), |request| {
        (request["method"] == "tools/call")
            .then(|| json!({ "content": [{ "type": "text", "text": "done" }] }))
    });
    let client = Client::new(transport);
    client.initialize().await.expect("handshake");

    let response = client
        .call_tool_task("work", None, TaskMetadata { ttl: None })
        .await
        .expect("a plain result is a success");

    assert!(
        matches!(response, CallToolResponse::Result(_)),
        "{response:?}"
    );
}

/// cancellation.mdx: the receiver of a cancellation SHOULD stop processing
/// and not respond. The handler of a server request the server cancelled
/// used to run to completion and answer anyway.
#[tokio::test]
async fn a_request_the_server_cancels_is_abandoned_without_a_response() {
    let (transport, server) = wire(json!({}), |_| None);
    let client = Client::new(transport);
    let handler = Arc::new(Hangs::default());
    client.set_elicitation_handler(handler.clone());
    client.initialize().await.expect("handshake");

    server.send(json!({
        "jsonrpc": "2.0",
        "id": "s-1",
        "method": "elicitation/create",
        "params": {
            "mode": "form",
            "message": "Pick a colour",
            "requestedSchema": { "type": "object", "properties": {} }
        }
    }));
    settle().await;
    assert!(handler.started.load(Ordering::SeqCst));

    server.send(json!({
        "jsonrpc": "2.0",
        "method": "notifications/cancelled",
        "params": { "requestId": "s-1", "reason": "no longer needed" }
    }));
    settle().await;

    assert!(
        handler.dropped.load(Ordering::SeqCst),
        "the handler must be aborted"
    );
    assert!(
        !server
            .received()
            .iter()
            .any(|m| m.get("method").is_none() && m["id"] == "s-1"),
        "a cancelled request gets no response: {:?}",
        server.received()
    );
}

/// `tasks/result` blocks until the task is terminal, so the request timeout
/// — 60s by default — failed every task that ran longer than it.
#[cfg(feature = "experimental-tasks")]
#[tokio::test]
async fn get_task_result_outlasts_the_request_timeout() {
    let (transport, _server) = scripted_wire(with_tool_tasks(), |request, server| {
        if request["method"] != "tasks/result" {
            return None;
        }
        let (request, server) = (request.clone(), server.clone());
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            server.reply(&request, json!({ "content": [] }));
        });
        None
    });
    let mut config = TransportConfig::default();
    config.timeouts.request = Some(Duration::from_millis(50));
    config.timeouts.total = Some(Duration::from_millis(100));
    let client = Client::new_with_config(transport, config);
    client.initialize().await.expect("handshake");

    client
        .get_task_result("t-1")
        .await
        .expect("tasks/result waits for the task, not the request timeout");
}

/// Streamable HTTP: a 404 for the session means "start a new session by
/// sending a new `InitializeRequest`". The client re-initializes with the
/// request it used before and retries the call once, instead of failing it
/// and every call after it.
#[tokio::test]
async fn an_expired_session_is_renewed_and_the_request_retried() {
    let (transport, server) = wire(json!({ "tools": {} }), |request| {
        (request["method"] == "tools/list").then(|| json!({ "tools": [] }))
    });
    let client = Client::new(transport);
    client.initialize().await.expect("handshake");

    server.expire_session();
    client
        .list_tools()
        .await
        .expect("the call succeeds on the new session");

    let initializes = server.received_method("initialize");
    assert_eq!(initializes.len(), 2, "{:?}", server.received());
    assert_eq!(initializes[0]["params"], initializes[1]["params"]);
    assert_eq!(server.received_method("notifications/initialized").len(), 2);
    assert_eq!(server.received_method("tools/list").len(), 2);
    assert!(client.is_initialized());
}
