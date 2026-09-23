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
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc;
use turbomcp_client::Client;
use turbomcp_client::handlers::{
    ElicitationCompleteHandler, ElicitationHandler, ElicitationRequest, ElicitationResponse,
    HandlerError, HandlerResult,
};
use turbomcp_protocol::MessageId;
use turbomcp_transport::{
    Transport, TransportCapabilities, TransportMessage, TransportMetrics, TransportResult,
    TransportState, TransportType,
};

type Responder = dyn Fn(&Value) -> Option<Value> + Send + Sync;

/// The server end of the in-memory wire.
#[derive(Clone)]
struct Server {
    sent: Arc<Mutex<Vec<Value>>>,
    inject: mpsc::UnboundedSender<Value>,
}

impl Server {
    /// Push a server-initiated message to the client.
    fn send(&self, message: Value) {
        self.inject.send(message).expect("client transport alive");
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
    sent: Arc<Mutex<Vec<Value>>>,
    respond: Box<Responder>,
    inject: mpsc::UnboundedSender<Value>,
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
    let sent = Arc::new(Mutex::new(Vec::new()));
    let (inject, incoming) = mpsc::unbounded_channel();
    let server = Server {
        sent: Arc::clone(&sent),
        inject: inject.clone(),
    };
    let respond = move |request: &Value| {
        if request["method"] == "initialize" {
            return Some(json!({
                "protocolVersion": turbomcp_protocol::PROTOCOL_VERSION,
                "capabilities": capabilities,
                "serverInfo": { "name": "scripted", "version": "1.0.0" }
            }));
        }
        respond(request)
    };
    (
        Wire {
            capabilities: TransportCapabilities::default(),
            sent,
            respond: Box::new(respond),
            inject,
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
        self.sent
            .lock()
            .expect("sent log poisoned")
            .push(message.clone());

        let is_request = message.get("method").is_some() && message.get("id").is_some();
        if is_request && let Some(reply) = (self.respond)(&message) {
            let envelope = if reply.get("error").is_some() {
                json!({ "jsonrpc": "2.0", "id": message["id"], "error": reply["error"] })
            } else {
                json!({ "jsonrpc": "2.0", "id": message["id"], "result": reply })
            };
            let _ = self.inject.send(envelope);
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
