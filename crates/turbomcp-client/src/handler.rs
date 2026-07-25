//! The client-serving handler — how a client answers server→client requests.
//!
//! A server can call *back* to the client: ask the user to fill a form
//! (`elicitation/create`), run an LLM sampling turn (`sampling/createMessage`),
//! or list the client's roots (`roots/list`). The user implements
//! [`ClientHandler`] to answer; the framework routes inbound requests to it on
//! both delivery models:
//!
//! - **Legacy inline bidi** — the request arrives as a real server→client
//!   JSON-RPC request mid-handler; the [`Connection`](crate::Connection) actor
//!   dispatches it here and writes the response back.
//! - **Draft MRTR** — the request is packaged into an `InputRequiredResult`; the
//!   [`Client`](crate::Client) MRTR loop pulls each packaged request, dispatches
//!   it here, and re-issues the original call with the gathered `inputResponses`.
//!
//! Both paths funnel through [`dispatch_server_request`], so a handler answers
//! identically regardless of version.
//!
//! `#[async_trait]` is used deliberately here (PLAN D5): the handler is a
//! cold-path, user-provided trait object — exactly the case native AFIT can't
//! store as `dyn`.

use async_trait::async_trait;
use serde_json::{Map, Value, json};
use turbomcp_core::JsonRpcError;
use turbomcp_protocol::methods::request;
use turbomcp_protocol::neutral;

use crate::error::{ClientError, ClientResult};

/// Answers server→client requests. Implement [`elicit`](ClientHandler::elicit)
/// at minimum; `sampling` and `roots` have defaults (most clients don't serve
/// them).
#[async_trait]
pub trait ClientHandler: Send + Sync + 'static {
    /// Answer an elicitation: present `request.message` + `request.requested_schema`
    /// to the user and return their [`ElicitOutcome`](neutral::ElicitOutcome).
    async fn elicit(&self, request: neutral::ElicitParams) -> neutral::ElicitOutcome;

    /// Answer a sampling request (`sampling/createMessage`). The default refuses
    /// — override to support LLM sampling. `params`/return are raw JSON until
    /// the Phase 9 sampling-typing pass.
    async fn create_message(&self, _params: Value) -> ClientResult<Value> {
        Err(ClientError::Protocol(
            "this client does not support sampling".into(),
        ))
    }

    /// Answer a `roots/list` request. The default returns an empty root list.
    async fn list_roots(&self) -> ClientResult<Value> {
        Ok(json!({ "roots": [] }))
    }

    /// Observe a server→client *notification* (`notifications/progress`,
    /// `notifications/message`, `*_list_changed`, `resources/updated`, …).
    /// Fire-and-forget — there is nothing to answer. The default ignores it.
    /// (The client's response cache is invalidated by `list_changed`
    /// notifications independently of this hook.)
    ///
    /// `notifications/elicitation/complete` also routes to the dedicated
    /// [`on_elicitation_complete`](Self::on_elicitation_complete) hook.
    async fn on_notification(&self, method: String, params: Option<Value>) {
        let _ = (method, params);
    }

    /// An out-of-band interaction started by a URL-mode `elicitation/create`
    /// finished (`notifications/elicitation/complete`): retry the request that
    /// needed it, dismiss the "waiting on you" UI, or ignore it.
    ///
    /// Per spec you **must ignore** ids you don't recognize or have already
    /// completed, and must not *rely* on this arriving — it is a server MAY,
    /// so keep the manual retry/cancel controls working regardless. The
    /// default ignores it.
    async fn on_elicitation_complete(&self, elicitation_id: String) {
        let _ = elicitation_id;
    }
}

/// Dispatch one server→client request (`method` + `params`) to `handler` and
/// return the JSON result value, or a JSON-RPC error to send back.
///
/// Shared by the inline-bidi path (actor) and the MRTR loop (client), so the two
/// delivery models answer identically.
pub(crate) async fn dispatch_server_request(
    handler: &dyn ClientHandler,
    method: &str,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    match method {
        request::ELICITATION_CREATE => {
            let elicit = parse_elicit_params(params)?;
            let outcome = handler.elicit(elicit).await;
            Ok(elicit_outcome_value(&outcome))
        }
        request::SAMPLING_CREATE_MESSAGE => handler
            .create_message(params.unwrap_or(Value::Null))
            .await
            .map_err(|e| internal_error(&e.to_string())),
        request::ROOTS_LIST => handler
            .list_roots()
            .await
            .map_err(|e| internal_error(&e.to_string())),
        other => Err(JsonRpcError {
            code: -32601,
            message: format!("method not found: {other}"),
            data: None,
        }),
    }
}

/// Parse an `elicitation/create` request's params into [`neutral::ElicitParams`].
fn parse_elicit_params(params: Option<Value>) -> Result<neutral::ElicitParams, JsonRpcError> {
    let params = params.ok_or_else(|| invalid_params("elicitation/create requires params"))?;
    let message = params
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let requested_schema = params
        .get("requestedSchema")
        .cloned()
        .unwrap_or_else(|| json!({}));
    Ok(neutral::ElicitParams::new(message, requested_schema))
}

/// The wire shape of an [`ElicitOutcome`](neutral::ElicitOutcome): `{ action,
/// content }`, where `content` is present only on `accept`.
fn elicit_outcome_value(outcome: &neutral::ElicitOutcome) -> Value {
    let action = match outcome.action {
        neutral::ElicitAction::Accept => "accept",
        neutral::ElicitAction::Decline => "decline",
        neutral::ElicitAction::Cancel => "cancel",
    };
    let mut obj = Map::new();
    obj.insert("action".into(), json!(action));
    if outcome.action == neutral::ElicitAction::Accept {
        obj.insert("content".into(), Value::Object(outcome.content.clone()));
    }
    Value::Object(obj)
}

fn invalid_params(msg: &str) -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message: msg.to_owned(),
        data: None,
    }
}

fn internal_error(msg: &str) -> JsonRpcError {
    JsonRpcError {
        code: -32603,
        message: msg.to_owned(),
        data: None,
    }
}
