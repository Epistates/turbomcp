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
//! `#[async_trait]` is used deliberately here: the handler is a
//! cold-path, user-provided trait object — exactly the case native AFIT can't
//! store as `dyn`.

use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;

use async_trait::async_trait;
use serde_json::{Map, Value, json};
use turbomcp_core::{JsonRpcError, ProtocolVersion};
use turbomcp_protocol::methods::request;
use turbomcp_protocol::neutral;

use crate::error::{ClientError, ClientResult};

/// What a handler knows about the server request it is answering, beyond the
/// request's own params: passed to the `*_with` handler methods, whose
/// defaults ignore it and call the plain ones.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct HandlerContext {
    /// The server's JSON-RPC request id (`2025-*`); `None` for a
    /// `2026-07-28` input request, which travels inside the call that needs
    /// it and has an [`input_key`](Self::input_key) instead.
    pub request_id: Option<turbomcp_core::RequestId>,
    /// The `inputRequests` key a `2026-07-28` input request was sent under.
    pub input_key: Option<String>,
    /// The revision this connection negotiated.
    pub protocol_version: ProtocolVersion,
    /// The request's `_meta`, as the server sent it (trace context, the
    /// related task, …).
    pub meta: Map<String, Value>,
    /// Fires when the server cancels the request (`notifications/cancelled`)
    /// or the connection closes; the handler's future is dropped then too,
    /// so this is for work it spawned.
    pub cancellation: turbomcp_core::CancellationToken,
}

impl HandlerContext {
    pub(crate) fn new(
        protocol_version: ProtocolVersion,
        params: Option<&Value>,
        cancellation: turbomcp_core::CancellationToken,
    ) -> Self {
        Self {
            request_id: None,
            input_key: None,
            protocol_version,
            meta: params
                .and_then(|p| p.get("_meta"))
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
            cancellation,
        }
    }

    pub(crate) fn with_request_id(mut self, id: turbomcp_core::RequestId) -> Self {
        self.request_id = Some(id);
        self
    }

    pub(crate) fn with_input_key(mut self, key: impl Into<String>) -> Self {
        self.input_key = Some(key.into());
        self
    }
}

/// Answers `elicitation/create`: registering one declares `elicitation.form`.
#[async_trait]
pub trait ElicitationHandler: Send + Sync + 'static {
    /// Present `request.message` + `request.requested_schema` to the user and
    /// return their [`ElicitOutcome`](neutral::ElicitOutcome). Declining is
    /// always valid.
    async fn elicit(&self, request: neutral::ElicitParams) -> neutral::ElicitOutcome;

    /// [`elicit`](Self::elicit), with what is known about the request. The
    /// one the client calls; the default calls `elicit`.
    ///
    /// An `Err` answers the server with that JSON-RPC error rather than a
    /// user's decision: for a client that can't put the question to anyone
    /// right now (a gateway whose caller can't elicit), which is not the
    /// same as the user cancelling.
    async fn elicit_with(
        &self,
        ctx: &HandlerContext,
        request: neutral::ElicitParams,
    ) -> ClientResult<neutral::ElicitOutcome> {
        let _ = ctx;
        Ok(self.elicit(request).await)
    }

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

    /// [`elicit_url`](Self::elicit_url), with what is known about the
    /// request. The default calls `elicit_url`.
    async fn elicit_url_with(
        &self,
        ctx: &HandlerContext,
        request: neutral::ElicitUrlParams,
    ) -> ClientResult<neutral::ElicitOutcome> {
        let _ = ctx;
        Ok(self.elicit_url(request).await)
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
    /// Run an LLM turn over `params.messages` and return the model's reply.
    ///
    /// Returning [`ClientError::Rpc`] lets the handler pick the JSON-RPC code
    /// the server sees; anything else becomes `-32603`.
    async fn create_message(
        &self,
        params: neutral::CreateMessageParams,
    ) -> ClientResult<neutral::CreateMessageResult>;

    /// [`create_message`](Self::create_message), with what is known about
    /// the request. The default calls `create_message`.
    async fn create_message_with(
        &self,
        ctx: &HandlerContext,
        params: neutral::CreateMessageParams,
    ) -> ClientResult<neutral::CreateMessageResult> {
        let _ = ctx;
        self.create_message(params).await
    }

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
    ///
    /// A [`Root`](neutral::Root) can only be built from a `file://` URI, which
    /// is the one shape rule the roots spec states — a root is a permission
    /// statement, and a server acting on `https://…` would be acting on a
    /// boundary this client never drew. A `Vec<Root>` converts with `.into()`.
    async fn list_roots(&self) -> ClientResult<neutral::ListRootsResult>;

    /// [`list_roots`](Self::list_roots), with what is known about the
    /// request. The default calls `list_roots`.
    async fn list_roots_with(
        &self,
        ctx: &HandlerContext,
    ) -> ClientResult<neutral::ListRootsResult> {
        let _ = ctx;
        self.list_roots().await
    }

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
///
/// Implement the typed hooks for the notifications you care about, or
/// [`on_notification`](Self::on_notification) to see every one as sent; a
/// notification reaches its typed hook (when it parses) and then
/// `on_notification`. All default to doing nothing.
#[async_trait]
pub trait NotificationHandler: Send + Sync + 'static {
    /// Observe every notification: `notifications/progress`,
    /// `notifications/message`, `*_list_changed`, `resources/updated`, and
    /// the rest, as sent. Fire-and-forget.
    ///
    /// The response cache is invalidated by `list_changed` notifications
    /// independently of this hook, and
    /// `notifications/elicitation/complete` also routes to
    /// [`ElicitationHandler::on_elicitation_complete`]. Progress for a call
    /// made with [`CallOptions::on_progress`](crate::CallOptions::on_progress)
    /// goes to that call instead of here.
    async fn on_notification(&self, method: String, params: Option<Value>) {
        let _ = (method, params);
    }

    /// A `notifications/message` log entry (sent once the client chose a
    /// level with `ClientBuilder::with_log_level` or `Client::set_level`).
    async fn on_log_message(&self, message: neutral::LogMessage) {
        let _ = message;
    }

    /// A `notifications/progress` update for `token` that no call claimed.
    async fn on_progress(&self, token: Value, progress: neutral::Progress) {
        let _ = (token, progress);
    }

    /// `notifications/resources/updated`: the resource at `uri` changed.
    async fn on_resource_updated(&self, uri: String) {
        let _ = uri;
    }

    /// `notifications/tools/list_changed`.
    async fn on_tools_list_changed(&self) {}

    /// `notifications/prompts/list_changed`.
    async fn on_prompts_list_changed(&self) {}

    /// `notifications/resources/list_changed`.
    async fn on_resources_list_changed(&self) {}
}

/// Hand `method` to `handler`'s typed hook, when it has one and the params
/// parse, then to its catch-all.
pub(crate) async fn deliver_notification(
    handler: &dyn NotificationHandler,
    method: String,
    params: Option<Value>,
) {
    use turbomcp_protocol::methods::notification;
    let p = params.as_ref();
    match method.as_str() {
        notification::MESSAGE => {
            if let Some(message) = p.and_then(neutral::LogMessage::from_params) {
                handler.on_log_message(message).await;
            }
        }
        notification::PROGRESS => {
            if let (Some(token), Some(progress)) = (
                p.and_then(|p| p.get("progressToken")).cloned(),
                p.and_then(neutral::Progress::from_params),
            ) {
                handler.on_progress(token, progress).await;
            }
        }
        notification::RESOURCES_UPDATED => {
            if let Some(uri) = p.and_then(|p| p.get("uri")).and_then(Value::as_str) {
                handler.on_resource_updated(uri.to_owned()).await;
            }
        }
        notification::TOOLS_LIST_CHANGED => handler.on_tools_list_changed().await,
        notification::PROMPTS_LIST_CHANGED => handler.on_prompts_list_changed().await,
        notification::RESOURCES_LIST_CHANGED => handler.on_resources_list_changed().await,
        _ => {}
    }
    handler.on_notification(method, params).await;
}

/// How many URL-mode elicitation ids a client remembers awaiting completion.
const MAX_OUTSTANDING_ELICITATIONS: usize = 1024;

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
    /// Client extensions: the `resultType`s and notifications they claim.
    pub(crate) extensions: Vec<Arc<dyn crate::ClientExtension>>,
    /// URL-mode elicitation ids this client has actually been sent and has not
    /// yet seen completed.
    ///
    /// "Clients MUST ignore completion notifications for unknown or
    /// already-completed elicitation IDs" — which needs a record of what was
    /// asked, and there was none: every `notifications/elicitation/complete`
    /// reached the handler, including one naming an id this client never saw.
    /// Shared across clones because the dispatcher and the notification router
    /// run on different tasks.
    /// In the order they arrived, so the oldest goes first at the bound.
    outstanding_elicitations: Arc<Mutex<VecDeque<String>>>,
}

impl core::fmt::Debug for ClientHandlers {
    /// Which features are served, never the user's trait objects.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ClientHandlers")
            .field("elicitation", &self.elicitation.is_some())
            .field("sampling", &self.sampling.is_some())
            .field("roots", &self.roots.is_some())
            .field("notifications", &self.notifications.is_some())
            .field(
                "extensions",
                &self.extensions.iter().map(|e| e.id()).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl ClientHandlers {
    /// These handlers, falling back to `global`'s for what they don't answer.
    pub(crate) fn over(&self, global: &ClientHandlers) -> ClientHandlers {
        ClientHandlers {
            elicitation: self
                .elicitation
                .clone()
                .or_else(|| global.elicitation.clone()),
            sampling: self.sampling.clone().or_else(|| global.sampling.clone()),
            roots: self.roots.clone().or_else(|| global.roots.clone()),
            ..global.clone()
        }
    }

    /// Whether these handlers answer the server→client request `method`.
    pub(crate) fn answers(&self, method: &str) -> bool {
        match method {
            request::ELICITATION_CREATE => self.elicitation.is_some(),
            request::SAMPLING_CREATE_MESSAGE => self.sampling.is_some(),
            request::ROOTS_LIST => self.roots.is_some(),
            _ => false,
        }
    }

    /// Whether any handler is registered (nothing answers server→client
    /// requests otherwise).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.elicitation.is_none()
            && self.sampling.is_none()
            && self.roots.is_none()
            && self.notifications.is_none()
    }

    /// The extension claiming `result_type`, if one does.
    pub(crate) fn claiming_result(
        &self,
        result_type: &str,
    ) -> Option<&Arc<dyn crate::ClientExtension>> {
        self.extensions
            .iter()
            .find(|e| e.result_types().contains(&result_type))
    }

    /// The extension claiming notification `method`, if one does.
    pub(crate) fn claiming_notification(
        &self,
        method: &str,
    ) -> Option<&Arc<dyn crate::ClientExtension>> {
        self.extensions
            .iter()
            .find(|e| e.notifications().contains(&method))
    }

    /// Note the `elicitationId` of an inbound URL-mode `elicitation/create`,
    /// so the paired `notifications/elicitation/complete` is recognized.
    ///
    /// Called from the *routing* step rather than the spawned dispatch task:
    /// the completion may follow the request immediately, and registering on
    /// the task would lose the race for exactly the server that is quickest to
    /// tell us the user is done.
    pub(crate) fn expect_elicitation(
        &self,
        version: &ProtocolVersion,
        method: &str,
        params: Option<&Value>,
    ) {
        // Only an id a completion could ever arrive for: `elicitation/complete`
        // exists on 2025-11-25 alone, and a URL-mode request this client will
        // refuse (it declared no URL mode) completes nothing. Recording those
        // grew the set without bound, at the server's pace.
        if method != request::ELICITATION_CREATE
            || *version != ProtocolVersion::V2025_11_25
            || !self
                .elicitation
                .as_ref()
                .is_some_and(|h| h.supports_url_mode())
        {
            return;
        }
        let Some(params) = params else { return };
        if params.get("mode").and_then(Value::as_str) != Some("url") {
            return;
        }
        if let Some(id) = params.get("elicitationId").and_then(Value::as_str) {
            let mut outstanding = self.outstanding_elicitations.lock();
            if outstanding.iter().any(|known| known == id) {
                return;
            }
            // A completion is a MAY, so some ids are never claimed; a bound
            // keeps a long-lived client from holding them all, and the one
            // that goes is the oldest (it used to be the smallest by string
            // order, which could be the newest).
            if outstanding.len() >= MAX_OUTSTANDING_ELICITATIONS {
                outstanding.pop_front();
            }
            outstanding.push_back(id.to_owned());
        }
    }

    /// Take `id` if it names an elicitation this client is still waiting on.
    ///
    /// `false` covers both halves of the spec's MUST: an id the server never
    /// sent us, and one a previous notification already completed.
    pub(crate) fn claim_elicitation(&self, id: &str) -> bool {
        let mut outstanding = self.outstanding_elicitations.lock();
        match outstanding.iter().position(|known| known == id) {
            Some(at) => outstanding.remove(at).is_some(),
            None => false,
        }
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
///
/// `version` is the session's negotiated revision, which decides how a result
/// is rendered — `2025-06-18` has no multi-block sampling content.
pub(crate) async fn dispatch_server_request(
    handlers: &ClientHandlers,
    ctx: &HandlerContext,
    method: &str,
    params: Option<Value>,
) -> Result<Value, JsonRpcError> {
    let version = &ctx.protocol_version;
    match method {
        request::ELICITATION_CREATE => {
            let handler = handlers
                .elicitation
                .as_ref()
                .ok_or_else(|| not_supported(method))?;
            // The wire discriminates on `mode`; absent means form (the shape
            // that predates URL mode). Anything else — a mode from a future
            // revision, or a typo like "URL" — is a mode this client did not
            // declare, which is the same answer as one it cannot present.
            //
            // "Server sends an `elicitation/create` request with a mode not
            // declared in client capabilities: -32602" is a client MUST on
            // both revisions that define modes. Declining instead tells the
            // server the *user* refused, which is a different and untrue fact.
            match params
                .as_ref()
                .and_then(|p| p.get("mode"))
                .map(|m| m.as_str())
            {
                None | Some(Some("form")) => {
                    let request = parse_elicit_params(params)?;
                    let schema = request.requested_schema.clone();
                    let outcome = handler
                        .elicit_with(ctx, request)
                        .await
                        .map_err(handler_error)?;
                    check_form_answer(&schema, &outcome)?;
                    Ok(outcome.to_wire(version))
                }
                Some(Some("url")) => {
                    if !handler.supports_url_mode() {
                        return Err(invalid_params(
                            "this client did not declare elicitation.url",
                        ));
                    }
                    let outcome = handler
                        .elicit_url_with(ctx, parse_elicit_url_params(params)?)
                        .await
                        .map_err(handler_error)?;
                    // A URL-mode answer carries no content: the form, if any,
                    // was filled in out of band.
                    let mut wire = outcome.to_wire(version);
                    if let Some(obj) = wire.as_object_mut() {
                        obj.remove("content");
                    }
                    Ok(wire)
                }
                Some(other) => Err(invalid_params(&format!(
                    "this client did not declare elicitation mode {}",
                    other.unwrap_or("<non-string>")
                ))),
            }
        }
        request::SAMPLING_CREATE_MESSAGE => {
            let handler = handlers
                .sampling
                .as_ref()
                .ok_or_else(|| not_supported(method))?;
            let params = neutral::CreateMessageParams::from_wire(&params.unwrap_or(Value::Null))
                .map_err(|e| invalid_params(&e.to_string()))?;
            // "The client MUST return an error if this field is provided but
            // ClientCapabilities.sampling.tools is not declared" — the server
            // is supposed to have checked, so this is the backstop for one
            // that did not. `includeContext` carries the same promise.
            let declared = handler.capability();
            if !declared.tools && params.uses_tools() {
                return Err(invalid_params("this client did not declare sampling.tools"));
            }
            if !declared.context && params.uses_context() {
                return Err(invalid_params(
                    "this client did not declare sampling.context",
                ));
            }
            // The conversation the server sent has to obey the tool-use MUSTs
            // too; answering an unbalanced one would hand the model a prompt
            // with a hole in it and blame the reply on this client.
            params
                .validate()
                .map_err(|e| invalid_params(&e.to_string()))?;
            let result = handler
                .create_message_with(ctx, params)
                .await
                .map_err(handler_error)?;
            result
                .to_wire(version)
                .map_err(|e| internal_error(&e.to_string()))
        }
        request::ROOTS_LIST => handlers
            .roots
            .as_ref()
            .ok_or_else(|| not_supported(method))?
            .list_roots_with(ctx)
            .await
            .map(|roots| roots.to_wire(version))
            .map_err(handler_error),
        other => Err(JsonRpcError {
            code: turbomcp_core::codes::METHOD_NOT_FOUND,
            message: format!("method not found: {other}"),
            data: None,
        }),
    }
}

/// Turn a handler's error into the JSON-RPC error to send back.
///
/// A handler that returned [`ClientError::Rpc`] chose a code deliberately;
/// flattening everything to `-32603` discarded it, so a handler could never
/// answer `-32602` for params it judged invalid. Anything else is genuinely
/// an internal failure of this client.
fn handler_error(err: ClientError) -> JsonRpcError {
    match err {
        ClientError::Rpc(rpc) => rpc,
        other => internal_error(&other.to_string()),
    }
}

/// A method the client never declared a handler for.
fn not_supported(method: &str) -> JsonRpcError {
    JsonRpcError {
        code: turbomcp_core::codes::METHOD_NOT_FOUND,
        message: format!("this client does not support {method}"),
        data: None,
    }
}

/// Parse a URL-mode `elicitation/create` request's params, refusing what the
/// spec requires and a handler shouldn't have to check: "The `url` parameter
/// MUST contain a valid URL", here an `http` or `https` one (a `file:`,
/// `javascript:` or `data:` URL is not somewhere to send a user), and the
/// `message` and `elicitationId` the schema requires.
fn parse_elicit_url_params(
    params: Option<Value>,
) -> Result<neutral::ElicitUrlParams, JsonRpcError> {
    let params = params.ok_or_else(|| invalid_params("elicitation/create requires params"))?;
    let field = |name: &str| {
        params.get(name).and_then(Value::as_str).ok_or_else(|| {
            invalid_params(&format!("url-mode elicitation/create requires `{name}`"))
        })
    };
    let message = field("message")?.to_owned();
    let id = field("elicitationId")?.to_owned();
    let url = field("url")?;
    let parsed = url::Url::parse(url)
        .map_err(|e| invalid_params(&format!("url-mode elicitation `url` is not a URL: {e}")))?;
    if !matches!(parsed.scheme(), "https" | "http") {
        return Err(invalid_params(&format!(
            "url-mode elicitation `url` must be http or https, not `{}:`",
            parsed.scheme()
        )));
    }
    Ok(neutral::ElicitUrlParams::new(message, url).with_elicitation_id(id))
}

/// Hold a form answer to the schema it was asked for ("Clients SHOULD
/// validate all responses against the provided schema"): a handler that
/// returns something else would hand the server data it never agreed to
/// take. A schema that doesn't compile can't be checked and isn't.
fn check_form_answer(schema: &Value, outcome: &neutral::ElicitOutcome) -> Result<(), JsonRpcError> {
    if outcome.action != neutral::ElicitAction::Accept {
        return Ok(());
    }
    let Ok(validator) = jsonschema::validator_for(schema) else {
        return Ok(());
    };
    validator
        .validate(&Value::Object(outcome.content.clone()))
        .map_err(|e| {
            internal_error(&format!(
                "the elicitation answer does not match requestedSchema: {e}"
            ))
        })
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

fn invalid_params(msg: &str) -> JsonRpcError {
    JsonRpcError {
        code: turbomcp_core::codes::INVALID_PARAMS,
        message: msg.to_owned(),
        data: None,
    }
}

fn internal_error(msg: &str) -> JsonRpcError {
    JsonRpcError {
        code: turbomcp_core::codes::INTERNAL_ERROR,
        message: msg.to_owned(),
        data: None,
    }
}

#[cfg(test)]
mod test_support {
    use super::{ClientHandlers, JsonRpcError, ProtocolVersion, Value, dispatch_server_request};

    /// Dispatch on `2025-11-25`, the widest of the stateful wires — the tests
    /// that care about the revision name it themselves.
    pub(super) async fn dispatch(
        handlers: &ClientHandlers,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, JsonRpcError> {
        let ctx = super::HandlerContext::new(
            ProtocolVersion::V2025_11_25,
            params.as_ref(),
            turbomcp_core::CancellationToken::new(),
        );
        dispatch_server_request(handlers, &ctx, method, params).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp_protocol::neutral::{ElicitationCapability, RootsCapability, SamplingCapability};

    use crate::handler::test_support::dispatch;

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
        async fn create_message(
            &self,
            _p: neutral::CreateMessageParams,
        ) -> ClientResult<neutral::CreateMessageResult> {
            Ok(neutral::CreateMessageResult::text("test-model", "ok"))
        }
        fn capability(&self) -> SamplingCapability {
            SamplingCapability::new().with_tools(true)
        }
    }
    #[async_trait]
    impl RootsHandler for Agentic {
        async fn list_roots(&self) -> ClientResult<neutral::ListRootsResult> {
            Ok(neutral::ListRootsResult::default())
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
            ..ClientHandlers::default()
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
        let err = dispatch(&handlers, request::SAMPLING_CREATE_MESSAGE, None)
            .await
            .expect_err("sampling was never registered");
        assert_eq!(err.code, -32601);
        let err = dispatch(&handlers, request::ROOTS_LIST, None)
            .await
            .expect_err("roots was never registered");
        assert_eq!(err.code, -32601);
    }
}

#[cfg(test)]
mod must_tests {
    use super::*;
    use turbomcp_protocol::neutral::SamplingCapability;

    use crate::handler::test_support::dispatch;

    struct FormClient;
    #[async_trait]
    impl ElicitationHandler for FormClient {
        async fn elicit(&self, _r: neutral::ElicitParams) -> neutral::ElicitOutcome {
            neutral::ElicitOutcome::new(neutral::ElicitAction::Accept, Map::new())
        }
    }

    struct PlainSampler;
    #[async_trait]
    impl SamplingHandler for PlainSampler {
        async fn create_message(
            &self,
            _p: neutral::CreateMessageParams,
        ) -> ClientResult<neutral::CreateMessageResult> {
            Ok(neutral::CreateMessageResult::text("m", "hello"))
        }
    }

    struct PickySampler;
    #[async_trait]
    impl SamplingHandler for PickySampler {
        async fn create_message(
            &self,
            _p: neutral::CreateMessageParams,
        ) -> ClientResult<neutral::CreateMessageResult> {
            Err(ClientError::Rpc(JsonRpcError {
                code: turbomcp_core::codes::INVALID_PARAMS,
                message: "messages must be non-empty".into(),
                data: None,
            }))
        }
        fn capability(&self) -> SamplingCapability {
            SamplingCapability::new().with_tools(true)
        }
    }

    fn form_only() -> ClientHandlers {
        ClientHandlers {
            elicitation: Some(Arc::new(FormClient)),
            ..ClientHandlers::default()
        }
    }

    /// A mode the client did not declare is `-32602`, not a decline.
    ///
    /// Declining says the *user* refused. The user was never asked, because
    /// this client cannot open a consent page — reporting that as a refusal
    /// tells the server something untrue and hides the misconfiguration.
    #[tokio::test]
    async fn an_undeclared_elicitation_mode_is_invalid_params() {
        let err = dispatch(
            &form_only(),
            request::ELICITATION_CREATE,
            Some(json!({ "mode": "url", "message": "Sign in", "url": "https://e.example" })),
        )
        .await
        .expect_err("a form-only client cannot present a URL");
        assert_eq!(err.code, -32602);

        // A mode from a future revision, or a typo, is equally undeclared —
        // falling through to the form branch would answer a question that was
        // not asked.
        for mode in [json!("URL"), json!("voice"), json!(7)] {
            let err = dispatch(
                &form_only(),
                request::ELICITATION_CREATE,
                Some(json!({ "mode": mode, "message": "?" })),
            )
            .await
            .expect_err("unknown mode");
            assert_eq!(err.code, -32602, "mode {mode}");
        }

        // Form mode still works, with or without the explicit discriminator.
        for params in [
            json!({ "message": "?" }),
            json!({ "mode": "form", "message": "?" }),
        ] {
            assert!(
                dispatch(&form_only(), request::ELICITATION_CREATE, Some(params))
                    .await
                    .is_ok()
            );
        }
    }

    /// Tool-enabled sampling is refused by a client that declared bare
    /// `sampling`: "The client MUST return an error if this field is provided
    /// but ClientCapabilities.sampling.tools is not declared."
    #[tokio::test]
    async fn tool_enabled_sampling_needs_the_declaration() {
        let plain = ClientHandlers {
            sampling: Some(Arc::new(PlainSampler)),
            ..ClientHandlers::default()
        };
        let tool = json!({ "name": "echo", "inputSchema": { "type": "object" } });
        for params in [
            json!({ "messages": [], "maxTokens": 8, "tools": [tool] }),
            json!({ "messages": [], "maxTokens": 8, "toolChoice": { "mode": "auto" } }),
            json!({ "messages": [], "maxTokens": 8, "includeContext": "allServers" }),
        ] {
            let err = dispatch(
                &plain,
                request::SAMPLING_CREATE_MESSAGE,
                Some(params.clone()),
            )
            .await
            .expect_err("undeclared sampling feature");
            assert_eq!(err.code, -32602, "{params}");
        }
        // What it did declare still works, and so does the safe context value.
        for params in [
            json!({ "messages": [], "maxTokens": 8 }),
            json!({ "messages": [], "maxTokens": 8, "includeContext": "none" }),
        ] {
            assert!(
                dispatch(&plain, request::SAMPLING_CREATE_MESSAGE, Some(params))
                    .await
                    .is_ok()
            );
        }
    }

    /// Params the revision cannot have produced are `-32602`, not a turn the
    /// model answers. `maxTokens` is required by every revision's schema, and
    /// an unbalanced tool conversation is a MUST on both sides.
    #[tokio::test]
    async fn malformed_sampling_params_are_refused_before_the_handler_runs() {
        let plain = ClientHandlers {
            sampling: Some(Arc::new(PlainSampler)),
            ..ClientHandlers::default()
        };
        let unanswered = json!({
            "maxTokens": 8,
            "messages": [{
                "role": "assistant",
                "content": [{ "type": "tool_use", "id": "c1", "name": "echo", "input": {} }],
            }],
        });
        for params in [
            json!({ "messages": [] }),
            json!({ "maxTokens": 8 }),
            unanswered,
        ] {
            let err = dispatch(
                &plain,
                request::SAMPLING_CREATE_MESSAGE,
                Some(params.clone()),
            )
            .await
            .expect_err("malformed sampling params");
            assert_eq!(err.code, -32602, "{params}");
        }
    }

    /// "Clients MUST ignore completion notifications for unknown or
    /// already-completed elicitation IDs."
    ///
    /// The client kept no record of which ids it had been sent, so every
    /// notification reached the handler — including one naming an id the
    /// server invented, which is how a handler gets talked into retrying a
    /// request nobody asked about.
    #[tokio::test]
    async fn only_elicitation_ids_this_client_was_sent_are_recognized() {
        struct UrlClient;
        #[async_trait]
        impl ElicitationHandler for UrlClient {
            async fn elicit(&self, _r: neutral::ElicitParams) -> neutral::ElicitOutcome {
                neutral::ElicitOutcome::new(neutral::ElicitAction::Decline, Map::new())
            }
            fn supports_url_mode(&self) -> bool {
                true
            }
        }

        let handlers = ClientHandlers {
            elicitation: Some(Arc::new(UrlClient)),
            ..ClientHandlers::default()
        };
        assert!(
            !handlers.claim_elicitation("never-sent"),
            "an id the server invented is unknown"
        );

        let params = json!({
            "mode": "url",
            "message": "Sign in",
            "url": "https://e.example",
            "elicitationId": "eid-1",
        });
        handlers.expect_elicitation(
            &ProtocolVersion::V2025_11_25,
            request::ELICITATION_CREATE,
            Some(&params),
        );
        dispatch(&handlers, request::ELICITATION_CREATE, Some(params.clone()))
            .await
            .expect("a url-mode client answers this");

        assert!(handlers.claim_elicitation("eid-1"), "this one was asked");
        assert!(
            !handlers.claim_elicitation("eid-1"),
            "and only once: the second notification is already-completed"
        );

        // No completion can come on 2026-07-28, which has no
        // `elicitation/complete`, so nothing is remembered there.
        handlers.expect_elicitation(
            &ProtocolVersion::V2026_07_28,
            request::ELICITATION_CREATE,
            Some(&params),
        );
        assert!(!handlers.claim_elicitation("eid-1"));
    }

    /// A client that declared no URL mode refuses URL-mode requests, so no
    /// completion can follow; remembering their ids grew the set at the
    /// server's pace.
    #[test]
    fn a_client_without_url_mode_remembers_no_elicitation_ids() {
        let handlers = ClientHandlers::default();
        handlers.expect_elicitation(
            &ProtocolVersion::V2025_11_25,
            request::ELICITATION_CREATE,
            Some(&json!({ "mode": "url", "elicitationId": "eid-2" })),
        );
        assert!(!handlers.claim_elicitation("eid-2"));
    }

    /// Roots that are not `file://` URIs never reach the server: the scheme is
    /// the one thing the roots spec makes a MUST, and a root is a permission
    /// statement rather than a hint.
    #[tokio::test]
    async fn only_file_uri_roots_go_on_the_wire() {
        struct Mixed;
        #[async_trait]
        impl RootsHandler for Mixed {
            async fn list_roots(&self) -> ClientResult<neutral::ListRootsResult> {
                Ok(vec![
                    neutral::Root::new("file:///workspace").expect("a file URI"),
                    neutral::Root::new("file:///tmp/scratch")
                        .expect("a file URI")
                        .with_name("scratch"),
                ]
                .into())
            }
        }
        assert!(
            neutral::Root::new("https://example.com").is_none(),
            "a non-file root cannot even be constructed"
        );

        let handlers = ClientHandlers {
            roots: Some(Arc::new(Mixed)),
            ..ClientHandlers::default()
        };
        let out = dispatch(&handlers, request::ROOTS_LIST, None)
            .await
            .expect("roots is registered");
        assert_eq!(
            out,
            json!({ "roots": [
                { "uri": "file:///workspace" },
                { "uri": "file:///tmp/scratch", "name": "scratch" },
            ]})
        );
    }

    /// A handler that chose a JSON-RPC code keeps it. Flattening every error to
    /// `-32603` meant a client could never answer `-32602` for params it
    /// judged invalid.
    #[tokio::test]
    async fn a_handlers_chosen_error_code_survives() {
        let picky = ClientHandlers {
            sampling: Some(Arc::new(PickySampler)),
            ..ClientHandlers::default()
        };
        let err = dispatch(
            &picky,
            request::SAMPLING_CREATE_MESSAGE,
            Some(json!({ "messages": [], "maxTokens": 8 })),
        )
        .await
        .expect_err("the handler rejected it");
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("non-empty"), "{}", err.message);
    }

    /// A url-mode request must carry an http(s) `url`, a `message` and an
    /// `elicitationId`; an accepted one is answered without `content`.
    #[tokio::test]
    async fn url_mode_requests_are_checked_and_answered_without_content() {
        struct Navigator;
        #[async_trait]
        impl ElicitationHandler for Navigator {
            async fn elicit(&self, _r: neutral::ElicitParams) -> neutral::ElicitOutcome {
                neutral::ElicitOutcome::new(neutral::ElicitAction::Decline, Map::new())
            }
            fn supports_url_mode(&self) -> bool {
                true
            }
            async fn elicit_url(&self, _r: neutral::ElicitUrlParams) -> neutral::ElicitOutcome {
                neutral::ElicitOutcome::new(neutral::ElicitAction::Accept, Map::new())
            }
        }
        let handlers = ClientHandlers {
            elicitation: Some(Arc::new(Navigator)),
            ..ClientHandlers::default()
        };
        let ask = |url: &str, id: Option<&str>| {
            let mut params = json!({ "mode": "url", "message": "Go", "url": url });
            if let Some(id) = id {
                params["elicitationId"] = json!(id);
            }
            params
        };
        for (params, why) in [
            (ask("file:///etc/passwd", Some("e")), "file:"),
            (ask("javascript:alert(1)", Some("e")), "javascript:"),
            (ask("not a url", Some("e")), "unparsable"),
            (ask("https://ok.example", None), "no elicitationId"),
        ] {
            let err = dispatch(&handlers, request::ELICITATION_CREATE, Some(params))
                .await
                .expect_err(why);
            assert_eq!(err.code, -32602, "{why}");
        }
        let answer = dispatch(
            &handlers,
            request::ELICITATION_CREATE,
            Some(ask("https://ok.example", Some("e"))),
        )
        .await
        .unwrap();
        assert_eq!(
            answer,
            json!({ "action": "accept" }),
            "url mode omits content"
        );
    }

    /// A form answer that breaks the schema it was asked for is not sent.
    #[tokio::test]
    async fn a_form_answer_must_match_its_schema() {
        struct Sloppy;
        #[async_trait]
        impl ElicitationHandler for Sloppy {
            async fn elicit(&self, _r: neutral::ElicitParams) -> neutral::ElicitOutcome {
                let mut content = Map::new();
                content.insert("age".into(), json!("old"));
                neutral::ElicitOutcome::new(neutral::ElicitAction::Accept, content)
            }
        }
        let handlers = ClientHandlers {
            elicitation: Some(Arc::new(Sloppy)),
            ..ClientHandlers::default()
        };
        let err = dispatch(
            &handlers,
            request::ELICITATION_CREATE,
            Some(json!({
                "message": "Age?",
                "requestedSchema": {
                    "type": "object",
                    "properties": { "age": { "type": "integer" } },
                },
            })),
        )
        .await
        .expect_err("refused");
        assert_eq!(err.code, -32603);
    }
}
