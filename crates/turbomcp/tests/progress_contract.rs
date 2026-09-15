//! Progress notifications: who gets a token, and what goes on the wire.
//!
//! The MCP progress utility makes progress opt-in and strictly token-scoped:
//! a client asks by putting `progressToken` in a request's `_meta`, and the
//! server may only reference tokens it was actually given. Before 3.5.0 the
//! token was never read off the request, and the only progress helper invented
//! one from the request id — notifications no client could match.

use std::sync::Arc;

use turbomcp::prelude::*;
use turbomcp_core::session::{McpSession, SessionFuture};

/// Records what a handler pushes to the client.
#[derive(Debug, Default)]
struct RecordingSession {
    notifications: std::sync::Mutex<Vec<(String, serde_json::Value)>>,
}

impl McpSession for RecordingSession {
    fn call<'a>(
        &'a self,
        _method: &'a str,
        _params: serde_json::Value,
    ) -> SessionFuture<'a, serde_json::Value> {
        Box::pin(async { Ok(serde_json::Value::Null) })
    }

    fn notify<'a>(&'a self, method: &'a str, params: serde_json::Value) -> SessionFuture<'a, ()> {
        Box::pin(async move {
            self.notifications
                .lock()
                .unwrap()
                .push((method.to_string(), params));
            Ok(())
        })
    }
}

#[derive(Clone)]
struct Slow;

#[server(name = "slow", version = "1.0.0")]
impl Slow {
    /// Reports progress unconditionally, the way a handler author would.
    #[tool]
    async fn work(&self, ctx: &RequestContext) -> McpResult<String> {
        ctx.report_progress(1.0, Some(3.0), Some("starting"))
            .await?;
        ctx.report_progress(3.0, Some(3.0), None).await?;
        Ok("done".into())
    }

    /// Reports only what the client asked for.
    #[tool]
    async fn conditional(&self, ctx: &RequestContext) -> McpResult<bool> {
        Ok(ctx.wants_progress())
    }
}

fn ctx_with(session: &Arc<RecordingSession>) -> RequestContext {
    RequestContext::stdio().with_session(session.clone() as Arc<dyn McpSession>)
}

async fn call_with_meta(
    tool: &str,
    meta: Option<serde_json::Value>,
    ctx: RequestContext,
) -> serde_json::Value {
    let mut params = serde_json::json!({ "name": tool, "arguments": {} });
    if let Some(meta) = meta {
        params["_meta"] = meta;
    }
    Slow.handle_request(
        serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": params }),
        ctx,
    )
    .await
    .unwrap()
}

// ── The token reaches the handler ──────────────────────────────────────────

#[tokio::test]
async fn progress_token_reaches_the_handler() {
    let session = Arc::new(RecordingSession::default());
    let response = call_with_meta(
        "conditional",
        Some(serde_json::json!({ "progressToken": "abc123" })),
        ctx_with(&session),
    )
    .await;

    assert_eq!(response["result"]["content"][0]["text"], "true");
}

#[tokio::test]
async fn absent_token_means_no_progress_wanted() {
    let session = Arc::new(RecordingSession::default());
    let response = call_with_meta("conditional", None, ctx_with(&session)).await;

    assert_eq!(response["result"]["content"][0]["text"], "false");
}

// ── What lands on the wire ─────────────────────────────────────────────────

#[tokio::test]
async fn progress_notifications_carry_the_clients_token() {
    let session = Arc::new(RecordingSession::default());
    call_with_meta(
        "work",
        Some(serde_json::json!({ "progressToken": "abc123" })),
        ctx_with(&session),
    )
    .await;

    let sent = session.notifications.lock().unwrap().clone();
    assert_eq!(
        sent.len(),
        2,
        "both report_progress calls must be delivered"
    );

    let (method, params) = &sent[0];
    assert_eq!(method, "notifications/progress");
    // The client's token verbatim — not the request id.
    assert_eq!(params["progressToken"], "abc123");
    assert_eq!(params["progress"], 1.0);
    assert_eq!(params["total"], 3.0);
    assert_eq!(params["message"], "starting");

    // `total` and `message` are omitted when not supplied, rather than null.
    let (_, second) = &sent[1];
    assert_eq!(second["progress"], 3.0);
    assert!(second.get("message").is_none());

    // Progress must increase across notifications for one token.
    assert!(
        second["progress"].as_f64().unwrap() > params["progress"].as_f64().unwrap(),
        "progress must increase"
    );
}

/// The heart of it: a server may not reference a token the client never issued.
#[tokio::test]
async fn no_token_means_no_notifications_at_all() {
    let session = Arc::new(RecordingSession::default());
    let response = call_with_meta("work", None, ctx_with(&session)).await;

    // The tool still succeeds — progress is optional, not required.
    assert!(response["error"].is_null());
    assert_ne!(response["result"]["isError"], true);
    assert!(
        session.notifications.lock().unwrap().is_empty(),
        "progress must be silent when the client did not ask for it"
    );
}

#[tokio::test]
async fn numeric_tokens_are_preserved_as_numbers() {
    let session = Arc::new(RecordingSession::default());
    call_with_meta(
        "work",
        Some(serde_json::json!({ "progressToken": 12345 })),
        ctx_with(&session),
    )
    .await;

    let sent = session.notifications.lock().unwrap().clone();
    // Spec types the token `string | number`; a number must not become "12345".
    assert_eq!(sent[0].1["progressToken"], 12345);
    assert!(sent[0].1["progressToken"].is_number());
}

#[tokio::test]
async fn malformed_tokens_are_ignored_not_rejected() {
    let session = Arc::new(RecordingSession::default());
    // An object is not a valid ProgressToken. The request must still succeed.
    let response = call_with_meta(
        "work",
        Some(serde_json::json!({ "progressToken": { "nope": true } })),
        ctx_with(&session),
    )
    .await;

    assert!(response["error"].is_null());
    assert!(session.notifications.lock().unwrap().is_empty());
}

#[tokio::test]
async fn progress_without_a_session_is_not_an_error() {
    // Unidirectional transport: the handler still completes normally.
    let response = call_with_meta(
        "work",
        Some(serde_json::json!({ "progressToken": "abc123" })),
        RequestContext::stdio(),
    )
    .await;

    assert!(response["error"].is_null());
    assert_ne!(response["result"]["isError"], true);
    assert_eq!(response["result"]["content"][0]["text"], "done");
}
