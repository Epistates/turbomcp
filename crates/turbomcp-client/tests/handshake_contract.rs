//! What the client does with the `initialize` result.
//!
//! Two of the three fields the server sends back used to go straight in the
//! bin: the negotiated `protocolVersion` was never compared against anything,
//! and `instructions` — the one handshake field written for the model rather
//! than the client — was unreachable to applications entirely.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::json;
use turbomcp_client::Client;
use turbomcp_protocol::MessageId;
use turbomcp_transport::{
    Transport, TransportCapabilities, TransportError, TransportMessage, TransportMetrics,
    TransportResult, TransportState, TransportType,
};

/// Answers `initialize` with whatever the test wants.
#[derive(Debug)]
struct ScriptedServer {
    capabilities: TransportCapabilities,
    initialize_result: serde_json::Value,
    sent: Arc<Mutex<Vec<serde_json::Value>>>,
    responses: Mutex<Vec<TransportMessage>>,
    disconnected: Arc<AtomicBool>,
}

/// What the test can still see after the transport has been handed to the
/// client, which takes it by value.
#[derive(Debug, Clone)]
struct Probe {
    sent: Arc<Mutex<Vec<serde_json::Value>>>,
    disconnected: Arc<AtomicBool>,
}

impl Probe {
    fn methods_sent(&self) -> Vec<String> {
        self.sent
            .lock()
            .expect("sent queue poisoned")
            .iter()
            .filter_map(|request| request["method"].as_str().map(str::to_owned))
            .collect()
    }

    fn hung_up(&self) -> bool {
        self.disconnected.load(Ordering::SeqCst)
    }
}

impl ScriptedServer {
    fn new(initialize_result: serde_json::Value) -> (Self, Probe) {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let disconnected = Arc::new(AtomicBool::new(false));
        let probe = Probe {
            sent: Arc::clone(&sent),
            disconnected: Arc::clone(&disconnected),
        };
        (
            Self {
                capabilities: TransportCapabilities::default(),
                initialize_result,
                sent,
                responses: Mutex::new(Vec::new()),
                disconnected,
            },
            probe,
        )
    }
}

impl Transport for ScriptedServer {
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
        self.disconnected.store(true, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }

    fn send(
        &self,
        message: TransportMessage,
    ) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
        let request: serde_json::Value = match serde_json::from_slice(&message.payload) {
            Ok(request) => request,
            Err(e) => {
                return Box::pin(
                    async move { Err(TransportError::SerializationFailed(e.to_string())) },
                );
            }
        };
        self.sent
            .lock()
            .expect("sent queue poisoned")
            .push(request.clone());

        if request.get("id").is_none() {
            return Box::pin(async { Ok(()) });
        }

        let response = json!({
            "jsonrpc": "2.0",
            "id": request["id"].clone(),
            "result": self.initialize_result.clone(),
        });
        let payload = serde_json::to_vec(&response).expect("response serializes");
        self.responses
            .lock()
            .expect("response queue poisoned")
            .push(TransportMessage::new(
                MessageId::from("response-1"),
                payload.into(),
            ));
        Box::pin(async { Ok(()) })
    }

    fn receive(
        &self,
    ) -> Pin<Box<dyn Future<Output = TransportResult<Option<TransportMessage>>> + Send + '_>> {
        let response = self
            .responses
            .lock()
            .expect("response queue poisoned")
            .pop();
        Box::pin(async move { Ok(response) })
    }

    fn metrics(&self) -> Pin<Box<dyn Future<Output = TransportMetrics> + Send + '_>> {
        Box::pin(async { TransportMetrics::default() })
    }
}

/// MCP §Version Negotiation: "If the client does not support the version in the
/// server's response, it SHOULD disconnect."
///
/// The client used to accept anything and then send 2025-11-25-shaped requests
/// the server could not honour, so the mismatch surfaced later as a string of
/// unrelated-looking per-request failures.
#[tokio::test]
async fn an_unsupported_negotiated_version_fails_the_handshake() {
    let (transport, probe) = ScriptedServer::new(json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "serverInfo": { "name": "ancient", "version": "0.1.0" }
    }));
    let client = Client::new(transport);

    let error = client
        .initialize()
        .await
        .expect_err("a version we do not speak must not be accepted");
    assert!(error.to_string().contains("2024-11-05"), "{error}");

    assert!(
        probe.hung_up(),
        "the spec says SHOULD disconnect, not SHOULD warn"
    );
    assert!(
        !probe
            .methods_sent()
            .contains(&"notifications/initialized".to_string()),
        "a failed handshake must not announce itself as complete"
    );
    assert!(!client.is_initialized());
}

/// A version we do speak is kept for the session, so a caller that did not
/// hold on to the `InitializeResult` can still answer which wire it is on.
#[tokio::test]
async fn a_supported_version_is_retained_for_the_session() {
    let (transport, _probe) = ScriptedServer::new(json!({
        "protocolVersion": "2025-06-18",
        "capabilities": {},
        "serverInfo": { "name": "older", "version": "1.0.0" }
    }));
    let client = Client::new(transport);

    let result = client.initialize().await.expect("2025-06-18 is supported");
    assert_eq!(result.protocol_version, "2025-06-18");
    assert_eq!(
        client.negotiated_protocol_version().as_deref(),
        Some("2025-06-18")
    );
}

/// `instructions` is the field the spec says MAY be added to the model's system
/// prompt. Dropping it meant a host built on this client could not pass the
/// server's own usage guidance to the model — the whole reason a server sets it.
#[tokio::test]
async fn server_instructions_reach_the_caller() {
    let (transport, _probe) = ScriptedServer::new(json!({
        "protocolVersion": turbomcp_protocol::PROTOCOL_VERSION,
        "capabilities": {},
        "serverInfo": { "name": "guided", "version": "1.0.0" },
        "instructions": "Call search before fetch."
    }));
    let client = Client::new(transport);

    let result = client.initialize().await.expect("handshake");
    assert_eq!(
        result.instructions.as_deref(),
        Some("Call search before fetch.")
    );
}

#[tokio::test]
async fn a_server_that_sends_no_instructions_yields_none() {
    let (transport, _probe) = ScriptedServer::new(json!({
        "protocolVersion": turbomcp_protocol::PROTOCOL_VERSION,
        "capabilities": {},
        "serverInfo": { "name": "terse", "version": "1.0.0" }
    }));
    let client = Client::new(transport);

    let result = client.initialize().await.expect("handshake");
    assert!(result.instructions.is_none());
}
