//! The client-serving handler — how a client answers server→client requests.
//!
//! A server can call *back* to the client: ask the user to fill a form
//! (`elicitation/create`), run an LLM sampling turn (`sampling/createMessage`),
//! or list the client's roots (`roots/list`). Each is its own trait —
//! [`ElicitationHandler`], [`SamplingHandler`], [`RootsHandler`], plus
//! [`NotificationHandler`] for the fire-and-forget direction — and **the ones a
//! client registers are exactly the capabilities it advertises**. There is no
//! separate capability list to keep in step, because a server must not send
//! what the client did not declare, and a declaration that disagrees with the
//! code is invisible to both sides.
//!
//! The framework routes inbound requests to the registered set on both
//! delivery models:
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

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Map, Value, json};
use turbomcp_core::JsonRpcError;
use turbomcp_protocol::methods::request;
use turbomcp_protocol::neutral;

use crate::error::ClientResult;

/// Answers `elicitation/create`: registering one declares `elicitation.form`.
#[async_trait]
pub trait ElicitationHandler: Send + Sync + 'static {
    /// Present `request.message` + `request.requested_schema` to the user and
    /// return their [`ElicitOutcome`](neutral::ElicitOutcome). Declining is
    /// always valid.
    async fn elicit(&self, request: neutral::ElicitParams) -> neutral::ElicitOutcome;

    /// Whether this client can also send the user out of band to a URL
    /// (`mode: "url"`), which declares `elicitation.url`.
    ///
    /// Defaults to `false`, and the direction of that default is deliberate:
    /// under-declaring costs a feature, over-declaring strands the user on a
    /// consent page the client never opens. Answering `true` means
    /// [`elicit_url`](Self::elicit_url) actually navigates somewhere.
    fn supports_url_mode(&self) -> bool {
        false
    }

    /// Present a URL-mode elicitation. Only called when
    /// [`supports_url_mode`](Self::supports_url_mode) is `true`; the default
    /// declines, which is the honest answer for a client that cannot navigate.
    async fn elicit_url(&self, request: neutral::ElicitUrlParams) -> neutral::ElicitOutcome {
        let _ = request;
        neutral::ElicitOutcome::new(neutral::ElicitAction::Decline, Map::new())
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

/// Answers `sampling/createMessage`: registering one declares `sampling`.
#[async_trait]
pub trait SamplingHandler: Send + Sync + 'static {
    /// Run an LLM turn. `params`/return are raw JSON until the sampling-typing
    /// pass lands.
    async fn create_message(&self, params: Value) -> ClientResult<Value>;

    /// Which sampling features this client honours, declared as
    /// `sampling.context` / `sampling.tools`.
    ///
    /// Both default to `false`. A server that sees no `tools` will not send
    /// `tools`/`toolChoice`, and one that sees no `context` will send only
    /// `includeContext: "none"` — so the default costs features rather than
    /// correctness, which is the right way round.
    fn capability(&self) -> neutral::SamplingCapability {
        neutral::SamplingCapability::default()
    }
}

/// Answers `roots/list`: registering one declares `roots`.
#[async_trait]
pub trait RootsHandler: Send + Sync + 'static {
    /// The roots this client exposes to the server.
    async fn list_roots(&self) -> ClientResult<Value>;

    /// Whether this client emits `notifications/roots/list_changed` when its
    /// roots change, declared as `roots.listChanged`.
    ///
    /// Answer `true` only if something actually calls
    /// [`Client::notify_roots_changed`](crate::Client::notify_roots_changed);
    /// a server that believes the notification is coming will stop re-polling.
    fn list_changed(&self) -> bool {
        false
    }
}

/// Observes server→client *notifications*. Declares no capability: receiving
/// notifications is not something a client opts into.
#[async_trait]
pub trait NotificationHandler: Send + Sync + 'static {
    /// Observe `notifications/progress`, `notifications/message`,
    /// `*_list_changed`, `resources/updated`, and the rest. Fire-and-forget.
    ///
    /// The response cache is invalidated by `list_changed` notifications
    /// independently of this hook, and
    /// `notifications/elicitation/complete` also routes to
    /// [`ElicitationHandler::on_elicitation_complete`].
    async fn on_notification(&self, method: String, params: Option<Value>);
}

/// The handlers a client registered, and the capability declaration derived
/// from them.
///
/// Registration *is* the declaration. A client cannot advertise elicitation it
/// cannot answer, nor answer sampling it never advertised, because there is one
/// source for both: this set. The server refuses to send what the client did
/// not declare (SEP-2322), so the two failure modes a hand-written capability
/// object produces — a handler the server never calls, and a call the handler
/// refuses — stop being expressible.
#[derive(Clone, Default)]
pub struct ClientHandlers {
    pub(crate) elicitation: Option<Arc<dyn ElicitationHandler>>,
    pub(crate) sampling: Option<Arc<dyn SamplingHandler>>,
    pub(crate) roots: Option<Arc<dyn RootsHandler>>,
    pub(crate) notifications: Option<Arc<dyn NotificationHandler>>,
}

impl core::fmt::Debug for ClientHandlers {
    /// Which features are served, never the user's trait objects.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ClientHandlers")
            .field("elicitation", &self.elicitation.is_some())
            .field("sampling", &self.sampling.is_some())
            .field("roots", &self.roots.is_some())
            .field("notifications", &self.notifications.is_some())
            .finish()
    }
}

impl ClientHandlers {
    /// Whether any handler is registered (nothing answers server→client
    /// requests otherwise).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.elicitation.is_none()
            && self.sampling.is_none()
            && self.roots.is_none()
            && self.notifications.is_none()
    }

    /// The capability declaration these handlers imply.
    #[must_use]
    pub fn capabilities(&self) -> neutral::ClientCapabilities {
        let mut caps = neutral::ClientCapabilities::new();
        caps.elicitation = self
            .elicitation
            .as_ref()
            .map(|h| neutral::ElicitationCapability::form().with_url(h.supports_url_mode()));
        caps.sampling = self.sampling.as_ref().map(|h| h.capability());
        caps.roots = self
            .roots
            .as_ref()
            .map(|h| neutral::RootsCapability::new().with_list_changed(h.list_changed()));
        caps
    }
}

/// Dispatch one server→client request (`method` + `params`) to `handlers` and
/// return the JSON result value, or a JSON-RPC error to send back.
///
/// Shared by the inline-bidi path (actor) and the MRTR loop (client), so the two
/// delivery models answer identically. A method whose handler is unregistered
/// answers `-32601`: the client never declared it, so the server should not
/// have asked, and claiming otherwise would hide the mismatch.
pub(crate) async fn dispatch_server_request(
    handlers: &ClientHandlers,
    method: &str,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    match method {
        request::ELICITATION_CREATE => {
            let handler = handlers
                .elicitation
                .as_ref()
                .ok_or_else(|| not_supported(method))?;
            // The wire discriminates on `mode`; absent means form (the shape
            // that predates URL mode).
            let is_url = params
                .as_ref()
                .and_then(|p| p.get("mode"))
                .and_then(Value::as_str)
                == Some("url");
            let outcome = if is_url {
                handler.elicit_url(parse_elicit_url_params(params)?).await
            } else {
                handler.elicit(parse_elicit_params(params)?).await
            };
            Ok(elicit_outcome_value(&outcome))
        }
        request::SAMPLING_CREATE_MESSAGE => handlers
            .sampling
            .as_ref()
            .ok_or_else(|| not_supported(method))?
            .create_message(params.unwrap_or(Value::Null))
            .await
            .map_err(|e| internal_error(&e.to_string())),
        request::ROOTS_LIST => handlers
            .roots
            .as_ref()
            .ok_or_else(|| not_supported(method))?
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

/// A method the client never declared a handler for.
fn not_supported(method: &str) -> JsonRpcError {
    JsonRpcError {
        code: -32601,
        message: format!("this client does not support {method}"),
        data: None,
    }
}

/// Parse a URL-mode `elicitation/create` request's params.
fn parse_elicit_url_params(
    params: Option<Value>,
) -> Result<neutral::ElicitUrlParams, JsonRpcError> {
    let params = params.ok_or_else(|| invalid_params("elicitation/create requires params"))?;
    let message = params
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let url = params
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_params("url-mode elicitation/create requires `url`"))?
        .to_owned();
    let mut out = neutral::ElicitUrlParams::new(message, url);
    if let Some(id) = params.get("elicitationId").and_then(Value::as_str) {
        out = out.with_elicitation_id(id);
    }
    Ok(out)
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

#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp_protocol::neutral::{ElicitationCapability, RootsCapability, SamplingCapability};

    struct FormOnly;
    #[async_trait]
    impl ElicitationHandler for FormOnly {
        async fn elicit(&self, _r: neutral::ElicitParams) -> neutral::ElicitOutcome {
            neutral::ElicitOutcome::new(neutral::ElicitAction::Decline, Map::new())
        }
    }

    struct Agentic;
    #[async_trait]
    impl ElicitationHandler for Agentic {
        async fn elicit(&self, _r: neutral::ElicitParams) -> neutral::ElicitOutcome {
            neutral::ElicitOutcome::new(neutral::ElicitAction::Decline, Map::new())
        }
        fn supports_url_mode(&self) -> bool {
            true
        }
    }
    #[async_trait]
    impl SamplingHandler for Agentic {
        async fn create_message(&self, _p: Value) -> ClientResult<Value> {
            Ok(json!({}))
        }
        fn capability(&self) -> SamplingCapability {
            SamplingCapability::new().with_tools(true)
        }
    }
    #[async_trait]
    impl RootsHandler for Agentic {
        async fn list_roots(&self) -> ClientResult<Value> {
            Ok(json!({ "roots": [] }))
        }
        fn list_changed(&self) -> bool {
            true
        }
    }

    /// The whole point of the split: what a client answers and what it
    /// advertises come from one place, so they cannot disagree.
    #[test]
    fn registration_is_the_declaration() {
        let mut handlers = ClientHandlers::default();
        assert_eq!(handlers.capabilities(), neutral::ClientCapabilities::new());

        handlers.elicitation = Some(Arc::new(FormOnly));
        let caps = handlers.capabilities();
        assert_eq!(caps.elicitation, Some(ElicitationCapability::form()));
        assert_eq!(caps.sampling, None, "unregistered stays undeclared");
        assert_eq!(caps.roots, None);
    }

    /// Sub-capabilities travel with their handler too, so a client that opts
    /// into URL mode, agentic sampling and roots notifications says so.
    #[test]
    fn sub_capabilities_come_from_the_handler() {
        let handlers = ClientHandlers {
            elicitation: Some(Arc::new(Agentic)),
            sampling: Some(Arc::new(Agentic)),
            roots: Some(Arc::new(Agentic)),
            notifications: None,
        };
        let caps = handlers.capabilities();
        assert_eq!(
            caps.elicitation,
            Some(ElicitationCapability::form().with_url(true))
        );
        assert_eq!(
            caps.sampling,
            Some(SamplingCapability::new().with_tools(true))
        );
        assert_eq!(
            caps.roots,
            Some(RootsCapability::new().with_list_changed(true))
        );
    }

    /// A request whose handler is unregistered is `-32601`, not a silent
    /// success: the client never declared it, so answering anything else would
    /// hide the fact that the server should not have asked.
    #[tokio::test]
    async fn an_unregistered_method_is_method_not_found() {
        let handlers = ClientHandlers {
            elicitation: Some(Arc::new(FormOnly)),
            ..ClientHandlers::default()
        };
        let err = dispatch_server_request(&handlers, request::SAMPLING_CREATE_MESSAGE, None)
            .await
            .expect_err("sampling was never registered");
        assert_eq!(err.code, -32601);
        let err = dispatch_server_request(&handlers, request::ROOTS_LIST, None)
            .await
            .expect_err("roots was never registered");
        assert_eq!(err.code, -32601);
    }
}
