//! An extension's share of a `subscriptions/listen`: agreed before the
//! acknowledgement, activated after it.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tower::{Service, ServiceExt};
use turbomcp_core::{
    Implementation, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, McpRequest,
    RequestContext, RequestId, codes,
};
use turbomcp_server::{
    Extension, ExtensionRequest, McpServerCore, MethodRouter, SubscribeOutcome, VersionDispatcher,
};
use turbomcp_service::Peer;

#[derive(Clone)]
struct Plain;

impl McpServerCore for Plain {
    fn server_info(&self) -> Implementation {
        Implementation::new("plain", "1.0.0")
    }
}

/// Agrees to an `eager` filter and pushes a notification the moment it is
/// activated.
#[derive(Default)]
struct Eager {
    activated: AtomicUsize,
}

#[async_trait]
impl Extension for Eager {
    fn id(&self) -> &'static str {
        "com.example/eager"
    }
    fn methods(&self) -> &'static [&'static str] {
        &[]
    }
    async fn dispatch(&self, _request: ExtensionRequest) -> JsonRpcMessage {
        unreachable!("no methods")
    }
    async fn on_subscribe(
        &self,
        _peer: &Peer,
        _subscription_id: &RequestId,
        notifications: &Value,
        _client_declared: bool,
        _context: &RequestContext,
    ) -> SubscribeOutcome {
        if notifications.get("eager").is_some() {
            SubscribeOutcome::Subscribed(json!({ "eager": true }))
        } else {
            SubscribeOutcome::NotApplicable
        }
    }
    async fn activate(
        &self,
        peer: &Peer,
        subscription_id: &RequestId,
        _accepted: &Value,
        _context: &RequestContext,
    ) {
        self.activated.fetch_add(1, Ordering::SeqCst);
        let note = JsonRpcNotification::new(
            "notifications/eager",
            Some(json!({
                "_meta": { "io.modelcontextprotocol/subscriptionId": subscription_id }
            })),
        );
        peer.offer(note.into());
    }
}

/// Refuses any listen that asks for `refuse`.
struct Refuser;

#[async_trait]
impl Extension for Refuser {
    fn id(&self) -> &'static str {
        "com.example/refuser"
    }
    fn methods(&self) -> &'static [&'static str] {
        &[]
    }
    async fn dispatch(&self, _request: ExtensionRequest) -> JsonRpcMessage {
        unreachable!("no methods")
    }
    async fn on_subscribe(
        &self,
        _peer: &Peer,
        _subscription_id: &RequestId,
        notifications: &Value,
        _client_declared: bool,
        _context: &RequestContext,
    ) -> SubscribeOutcome {
        if notifications.get("refuse").is_some() {
            SubscribeOutcome::MissingCapability
        } else {
            SubscribeOutcome::NotApplicable
        }
    }
}

async fn listen(
    eager: Arc<Eager>,
    notifications: Value,
) -> (Option<JsonRpcMessage>, mpsc::Receiver<JsonRpcMessage>) {
    let mut svc = VersionDispatcher::new(Plain, MethodRouter::new())
        .with_extension(eager)
        .with_extension(Arc::new(Refuser));
    let (tx, rx) = mpsc::channel(16);
    let peer = Peer::new("ext-subs", &tx);
    let request = JsonRpcRequest::new(
        7,
        "subscriptions/listen",
        Some(json!({
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
            },
            "notifications": notifications,
        })),
    );
    let reply = svc
        .ready()
        .await
        .unwrap()
        .call(
            McpRequest::new(request)
                .with(peer.id().clone())
                .with(peer.clone()),
        )
        .await
        .unwrap();
    (reply, rx)
}

async fn next(rx: &mut mpsc::Receiver<JsonRpcMessage>) -> JsonRpcNotification {
    match tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
        Ok(Some(JsonRpcMessage::Notification(n))) => n,
        other => panic!("expected a notification, got {other:?}"),
    }
}

/// "The server MUST NOT send any notification on the subscription before"
/// its acknowledgement. An extension that pushes as soon as it may still
/// comes second.
#[tokio::test]
async fn an_extension_starts_after_the_acknowledgement() {
    let eager = Arc::new(Eager::default());
    let (reply, mut rx) = listen(Arc::clone(&eager), json!({ "eager": true })).await;
    assert!(reply.is_none());
    let first = next(&mut rx).await;
    assert_eq!(first.method, "notifications/subscriptions/acknowledged");
    assert_eq!(first.params.unwrap()["notifications"]["eager"], true);
    assert_eq!(next(&mut rx).await.method, "notifications/eager");
    assert_eq!(eager.activated.load(Ordering::SeqCst), 1);
}

/// A listen another extension refuses activates nothing: registering as each
/// extension answered left the earlier ones subscribed to a stream that was
/// never opened.
#[tokio::test]
async fn a_refused_listen_activates_no_extension() {
    let eager = Arc::new(Eager::default());
    let (reply, _rx) = listen(Arc::clone(&eager), json!({ "eager": true, "refuse": true })).await;
    let Some(JsonRpcMessage::Response(response)) = reply else {
        panic!("expected the refusal");
    };
    assert_eq!(
        response.error.unwrap().code,
        codes::MISSING_REQUIRED_CLIENT_CAPABILITY
    );
    assert_eq!(eager.activated.load(Ordering::SeqCst), 0);
}
