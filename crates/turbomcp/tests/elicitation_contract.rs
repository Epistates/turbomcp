//! What a server may put in a form elicitation, and what it accepts back.
//!
//! The spec limits form schemas to flat objects of primitive properties, and
//! says a server SHOULD validate what the client returns against the schema it
//! asked with. `elicit_form` used to send any JSON it was handed and return
//! whatever came back as a success.

use std::sync::{Arc, Mutex};

use serde_json::json;
use turbomcp_core::context::RequestContext;
use turbomcp_core::session::{McpSession, SessionFuture};
use turbomcp_types::{ClientCapabilities, ElicitAction, ElicitationCapabilities, ProtocolVersion};

/// A client that answers every elicitation with `answer`.
#[derive(Debug)]
struct FakeClient {
    version: ProtocolVersion,
    answer: serde_json::Value,
    sent: Mutex<Vec<serde_json::Value>>,
}

impl FakeClient {
    fn new(version: ProtocolVersion, answer: serde_json::Value) -> Arc<Self> {
        Arc::new(Self {
            version,
            answer,
            sent: Mutex::new(Vec::new()),
        })
    }
}

impl McpSession for FakeClient {
    fn client_capabilities<'a>(&'a self) -> SessionFuture<'a, Option<ClientCapabilities>> {
        Box::pin(async {
            Ok(Some(ClientCapabilities {
                elicitation: Some(ElicitationCapabilities::default()),
                ..Default::default()
            }))
        })
    }

    fn protocol_version<'a>(&'a self) -> SessionFuture<'a, Option<ProtocolVersion>> {
        let version = self.version.clone();
        Box::pin(async move { Ok(Some(version)) })
    }

    fn call<'a>(
        &'a self,
        _method: &'a str,
        params: serde_json::Value,
    ) -> SessionFuture<'a, serde_json::Value> {
        self.sent.lock().unwrap().push(params);
        let answer = self.answer.clone();
        Box::pin(async move { Ok(answer) })
    }

    fn notify<'a>(&'a self, _method: &'a str, _params: serde_json::Value) -> SessionFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}

fn ctx_for(session: &Arc<FakeClient>) -> RequestContext {
    RequestContext::new().with_session(session.clone() as Arc<dyn McpSession>)
}

fn name_form() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "age": { "type": "integer", "minimum": 0 }
        },
        "required": ["name"]
    })
}

#[tokio::test]
async fn a_valid_answer_is_returned() {
    let client = FakeClient::new(
        ProtocolVersion::V2025_11_25,
        json!({ "action": "accept", "content": { "name": "Ada", "age": 36 } }),
    );
    let result = ctx_for(&client)
        .elicit_form("Who are you?", name_form())
        .await
        .expect("a conforming answer");
    assert_eq!(result.action, ElicitAction::Accept);

    // Form is the default mode, and 2025-06-18 has no `mode` at all.
    let sent = client.sent.lock().unwrap();
    assert!(sent[0].get("mode").is_none(), "{}", sent[0]);
}

#[tokio::test]
async fn an_answer_missing_a_required_field_is_refused() {
    let client = FakeClient::new(
        ProtocolVersion::V2025_11_25,
        json!({ "action": "accept", "content": { "age": 36 } }),
    );
    let error = ctx_for(&client)
        .elicit_form("Who are you?", name_form())
        .await
        .expect_err("`name` is required");
    assert_eq!(error.jsonrpc_error_code(), -32602, "{error}");
}

#[tokio::test]
async fn an_answer_of_the_wrong_type_is_refused() {
    let client = FakeClient::new(
        ProtocolVersion::V2025_11_25,
        json!({ "action": "accept", "content": { "name": "Ada", "age": "old" } }),
    );
    let error = ctx_for(&client)
        .elicit_form("Who are you?", name_form())
        .await
        .expect_err("`age` is an integer");
    assert!(error.to_string().contains("age"), "{error}");
}

/// A decline carries no content, so there is nothing to validate.
#[tokio::test]
async fn a_decline_is_returned_as_is() {
    let client = FakeClient::new(ProtocolVersion::V2025_11_25, json!({ "action": "decline" }));
    let result = ctx_for(&client)
        .elicit_form("Who are you?", name_form())
        .await
        .expect("declining is a valid answer");
    assert_eq!(result.action, ElicitAction::Decline);
}

/// "Schemas are limited to flat objects with primitive properties only."
#[tokio::test]
async fn a_nested_schema_is_never_sent() {
    let client = FakeClient::new(ProtocolVersion::V2025_11_25, json!({ "action": "cancel" }));
    let error = ctx_for(&client)
        .elicit_form(
            "Where do you live?",
            json!({
                "type": "object",
                "properties": {
                    "address": { "type": "object", "properties": { "city": { "type": "string" } } }
                }
            }),
        )
        .await
        .expect_err("nested objects are not a form");
    assert_eq!(error.jsonrpc_error_code(), -32602, "{error}");
    assert!(client.sent.lock().unwrap().is_empty());
}

/// Multi-select arrived in 2025-11-25; a 2025-06-18 client has no rendering
/// for it.
#[tokio::test]
async fn a_multi_select_is_not_sent_to_a_2025_06_18_client() {
    let client = FakeClient::new(ProtocolVersion::V2025_06_18, json!({ "action": "cancel" }));
    let error = ctx_for(&client)
        .elicit_form(
            "Pick colours",
            json!({
                "type": "object",
                "properties": {
                    "colours": { "type": "array", "items": { "type": "string", "enum": ["r", "g"] } }
                }
            }),
        )
        .await
        .expect_err("2025-06-18 has no multi-select");
    assert!(error.to_string().contains("colours"), "{error}");
    assert!(client.sent.lock().unwrap().is_empty());
}
