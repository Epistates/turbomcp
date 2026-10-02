//! Both halves of an extension: a server extension answers `tools/call` with
//! a `resultType` of its own (and a notification), and the client extension
//! that claims it settles the result and receives the notification.
#![cfg(feature = "client")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value, json};
use turbomcp::client::{
    Client, ClientBuilder, ClientExtension, ClientResult, ConnectMode, async_trait,
};
use turbomcp::prelude::*;
use turbomcp::{
    CallAugmentRequest, Extension, ExtensionRequest, JsonRpcMessage, JsonRpcNotification,
    JsonRpcResponse, Peer,
};

const ID: &str = "com.example/deferred";
const DEFERRED: &str = "com.example/deferred";
const FETCH: &str = "deferred/fetch";
const READY: &str = "notifications/deferred/ready";

#[derive(Clone)]
struct Slow;

#[server(name = "slow", version = "1.0.0")]
impl Slow {
    /// Compute an answer.
    #[tool]
    async fn answer(&self) -> String {
        "42".into()
    }
}

/// Runs each call, keeps its result under a token, and answers with the
/// token instead; `deferred/fetch` hands the result over.
#[derive(Default)]
struct Deferring {
    results: Mutex<HashMap<String, Value>>,
}

#[async_trait]
impl Extension for Deferring {
    fn id(&self) -> &'static str {
        ID
    }
    fn methods(&self) -> &'static [&'static str] {
        &[FETCH]
    }
    fn augments_calls(&self) -> bool {
        true
    }
    async fn augment_call(&self, augment: CallAugmentRequest) -> Option<JsonRpcMessage> {
        let result = augment.run.run().await.ok()?;
        let token = "t1".to_owned();
        self.results.lock().unwrap().insert(token.clone(), result);
        if let Some(peer) = augment.context.extensions.get::<Peer>() {
            let note = JsonRpcNotification::new(READY, Some(json!({ "token": token })));
            let _ = peer.send(note.into()).await;
        }
        Some(
            JsonRpcResponse::success(
                augment.request.id,
                json!({ "resultType": DEFERRED, "token": token }),
            )
            .into(),
        )
    }
    async fn dispatch(&self, request: ExtensionRequest) -> JsonRpcMessage {
        let token = request
            .request
            .params
            .as_ref()
            .and_then(|p| p.get("token"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let result = self
            .results
            .lock()
            .unwrap()
            .remove(token)
            .unwrap_or_default();
        JsonRpcResponse::success(request.request.id, result).into()
    }
}

/// Claims the deferred result type and the ready notification.
#[derive(Default)]
struct Fetching {
    ready: Mutex<Vec<Value>>,
}

#[async_trait]
impl ClientExtension for Fetching {
    fn id(&self) -> &str {
        ID
    }
    fn result_types(&self) -> &[&str] {
        &[DEFERRED]
    }
    async fn settle(&self, client: &Client, _method: &str, result: Value) -> ClientResult<Value> {
        let mut params = Map::new();
        params.insert("token".into(), result["token"].clone());
        client.request(FETCH, params).await
    }
    fn notifications(&self) -> &[&str] {
        &[READY]
    }
    async fn on_notification(&self, _method: &str, params: Option<Value>) {
        self.ready.lock().unwrap().push(params.unwrap_or_default());
    }
}

#[tokio::test]
async fn a_claimed_result_type_settles_and_its_notification_arrives() {
    let fetching = Arc::new(Fetching::default());
    let client = turbomcp::testing::connect(
        Slow.into_server()
            .with_extension(Arc::new(Deferring::default())),
        ClientBuilder::new("t", "1.0.0")
            .with_connect_mode(ConnectMode::Modern)
            .with_client_extension(Arc::clone(&fetching) as Arc<dyn ClientExtension>),
    )
    .await
    .expect("handshake");
    let result = client
        .call_tool("answer", Map::new())
        .await
        .expect("settled");
    assert_eq!(result.text_content().as_deref(), Some("42"));
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while fetching.ready.lock().unwrap().is_empty() {
        assert!(tokio::time::Instant::now() < deadline, "no notification");
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(fetching.ready.lock().unwrap()[0]["token"], "t1");
}

/// Without the client extension the same answer is refused: "A `resultType`
/// of any value unrecognized by the client MUST be considered invalid."
#[tokio::test]
async fn an_unclaimed_result_type_is_refused() {
    let client = turbomcp::testing::connect(
        Slow.into_server()
            .with_extension(Arc::new(Deferring::default())),
        ClientBuilder::new("t", "1.0.0")
            .with_connect_mode(ConnectMode::Modern)
            .with_extension(ID, json!({})),
    )
    .await
    .expect("handshake");
    let err = client.call_tool("answer", Map::new()).await.unwrap_err();
    assert!(err.to_string().contains("unrecognized resultType"), "{err}");
}

#[test]
#[should_panic(expected = "both claim resultType")]
fn two_extensions_claiming_one_result_type_are_refused() {
    struct Other;
    #[async_trait]
    impl ClientExtension for Other {
        fn id(&self) -> &str {
            "com.example/other"
        }
        fn result_types(&self) -> &[&str] {
            &[DEFERRED]
        }
    }
    let _ = ClientBuilder::new("t", "1.0.0")
        .with_client_extension(Arc::new(Fetching::default()))
        .with_client_extension(Arc::new(Other));
}
