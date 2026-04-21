//! Verifies the v3.2 RequestContext unification: `#[tool]` handlers can reach
//! bidirectional session operations (`sample`, `elicit_form`, `elicit_url`,
//! `notify_client`) through the same `&RequestContext` the macro hands them.
//!
//! Before the unification the server built a rich `turbomcp_server::RequestContext`
//! (with the session handle), then called `to_core_context()` which dropped the
//! session before dispatching to tool bodies. This test guards that regression.

use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{Value, json};
use turbomcp_core::context::RequestContext;
use turbomcp_core::session::{McpSession, SessionFuture};

/// A test double that records every server→client call / notify and replies
/// with canned JSON.
#[derive(Debug, Default)]
struct RecordingSession {
    calls: Mutex<Vec<(String, Value)>>,
    notifications: Mutex<Vec<(String, Value)>>,
    canned_response: Mutex<Option<Value>>,
}

impl RecordingSession {
    fn new() -> Self {
        Self::default()
    }

    fn set_canned_response(&self, response: Value) {
        *self.canned_response.lock() = Some(response);
    }

    fn calls(&self) -> Vec<(String, Value)> {
        self.calls.lock().clone()
    }

    fn notifications(&self) -> Vec<(String, Value)> {
        self.notifications.lock().clone()
    }
}

impl McpSession for RecordingSession {
    fn call<'a>(&'a self, method: &'a str, params: Value) -> SessionFuture<'a, Value> {
        Box::pin(async move {
            self.calls.lock().push((method.to_string(), params));
            Ok(self
                .canned_response
                .lock()
                .clone()
                .unwrap_or_else(|| json!({})))
        })
    }

    fn notify<'a>(&'a self, method: &'a str, params: Value) -> SessionFuture<'a, ()> {
        Box::pin(async move {
            self.notifications.lock().push((method.to_string(), params));
            Ok(())
        })
    }
}

#[tokio::test]
async fn ctx_elicit_form_reaches_session() {
    let session = Arc::new(RecordingSession::new());
    session.set_canned_response(json!({ "action": "accept", "content": { "ok": true } }));

    let ctx = RequestContext::http().with_session(session.clone() as Arc<dyn McpSession>);

    let schema = json!({ "type": "object", "properties": {} });
    let result = ctx
        .elicit_form("Approve dangerous op?", schema.clone())
        .await
        .expect("elicit_form should succeed with a session");

    assert_eq!(result.action, turbomcp_types::ElicitAction::Accept);
    assert_eq!(session.calls().len(), 1);
    let (method, params) = &session.calls()[0];
    assert_eq!(method, "elicitation/create");
    assert_eq!(params["mode"], "form");
    assert_eq!(params["message"], "Approve dangerous op?");
    assert_eq!(params["requestedSchema"], schema);
}

#[tokio::test]
async fn ctx_elicit_url_reaches_session() {
    let session = Arc::new(RecordingSession::new());
    session.set_canned_response(json!({ "action": "accept" }));

    let ctx = RequestContext::http().with_session(session.clone() as Arc<dyn McpSession>);

    let _ = ctx
        .elicit_url("Finish OAuth", "https://example/cb", "elicit-1")
        .await
        .expect("elicit_url should succeed");

    let (method, params) = &session.calls()[0];
    assert_eq!(method, "elicitation/create");
    assert_eq!(params["mode"], "url");
    assert_eq!(params["url"], "https://example/cb");
    assert_eq!(params["elicitationId"], "elicit-1");
}

#[tokio::test]
async fn ctx_notify_client_reaches_session() {
    let session = Arc::new(RecordingSession::new());
    let ctx = RequestContext::http().with_session(session.clone() as Arc<dyn McpSession>);

    ctx.notify_client("notifications/tools/list_changed", json!({}))
        .await
        .expect("notify_client should succeed");

    assert_eq!(session.notifications().len(), 1);
    assert_eq!(
        session.notifications()[0].0,
        "notifications/tools/list_changed"
    );
}

#[tokio::test]
async fn ctx_sample_fails_without_session() {
    let ctx = RequestContext::stdio();
    let err = ctx
        .sample(turbomcp_types::CreateMessageRequest::default())
        .await
        .expect_err("sample should fail on context without a session");
    assert_eq!(
        err.kind,
        turbomcp_core::error::ErrorKind::CapabilityNotSupported
    );
}

#[tokio::test]
async fn ctx_notify_client_fails_without_session() {
    let ctx = RequestContext::stdio();
    let err = ctx
        .notify_client("notifications/progress", json!({}))
        .await
        .expect_err("notify_client should fail without a session");
    assert_eq!(
        err.kind,
        turbomcp_core::error::ErrorKind::CapabilityNotSupported
    );
}
