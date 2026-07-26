//! MRTR coordinator + [`ClientHandle`] (SEP-2322, PLAN §4.5.2).
//!
//! On the draft, server→client interaction (elicitation, sampling, roots) is
//! *not* a separate request: the handler records what it needs, aborts with
//! the [`McpError::InputRequired`] sentinel, and the dispatcher answers an
//! `InputRequiredResult`. The client gathers responses and **re-issues the
//! original request from the top** with `inputResponses` (+ the echoed
//! `requestState`); on re-execution the handle finds the cached response and
//! returns it inline. Handlers must therefore keep elicit keys stable and any
//! pre-elicit side effects idempotent (PLAN §4.5.1).
//!
//! `requestState` is the handler's opaque resume blob. It round-trips through
//! the client, so it is attacker-controlled input (mrtr spec MUST): outbound
//! state is HMAC-SHA256-signed and the protected payload binds the method name,
//! the authenticated principal (a state minted for one subject can't be
//! replayed by another), and an expiry; inbound state that fails any check is
//! rejected with `-32602` before the handler runs. The signing key defaults to
//! a per-dispatcher secret; multi-replica deployments share one via
//! [`ServerBuilder::with_state_key`](crate::ServerBuilder::with_state_key).
//!
//! On `2025-11-25` the same handle calls go out as **inline bidirectional
//! requests**: a real `elicitation/create` (etc.) JSON-RPC request is written
//! to the session's server→client channel and the handler blocks until the
//! client's response routes back through [`PendingRequests`]. No re-execution
//! happens on this path — handlers written for MRTR re-entry work unchanged.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit as _, Mac};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};
use sha2::Sha256;
use tokio::sync::oneshot;
use turbomcp_core::{JsonRpcRequest, JsonRpcResponse, McpError, McpResult, RequestId};
use turbomcp_protocol::methods::request;
use turbomcp_protocol::neutral;

use crate::subscriptions::request_writer;

type HmacSha256 = Hmac<Sha256>;

/// Cap on the serialized `requestState` payload (PLAN MR-5).
pub(crate) const MAX_STATE_BYTES: usize = 32 * 1024;
/// How long an issued `requestState` stays redeemable (replay bound — the mrtr
/// spec's SHOULD; single-use semantics, if needed, are the handler's job).
const STATE_TTL: Duration = Duration::from_secs(10 * 60);

// ---- request-state signing -----------------------------------------------------

/// Signs/verifies `requestState` blobs.
///
/// The key defaults to a per-dispatcher random secret, which is right for a
/// single process: nothing else can mint a state it will accept, and a restart
/// invalidates every outstanding one. A deployment running more than one
/// replica must supply a shared key instead — see
/// [`ServerBuilder::with_state_key`](crate::ServerBuilder::with_state_key).
pub(crate) struct StateSigner {
    key: [u8; 32],
}

impl StateSigner {
    pub(crate) fn new() -> Self {
        use rand::Rng as _;
        let mut key = [0u8; 32];
        rand::rng().fill_bytes(&mut key);
        Self { key }
    }

    /// A signer over a caller-supplied key (shared across replicas).
    pub(crate) fn from_key(key: [u8; 32]) -> Self {
        Self { key }
    }

    fn mac(&self) -> HmacSha256 {
        HmacSha256::new_from_slice(&self.key).expect("HMAC accepts any key length")
    }

    /// Wrap handler `data` into the opaque wire string:
    /// `v1.<b64url(payload)>.<b64url(tag)>` where the payload binds the
    /// originating `method`, the authenticated `subject` (principal binding —
    /// a state minted for one principal can't be replayed by another), and an
    /// expiry alongside the data. `subject` is `None` for an unauthenticated
    /// request.
    pub(crate) fn sign(
        &self,
        method: &str,
        subject: Option<&str>,
        data: &Value,
    ) -> McpResult<String> {
        let expires = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            + STATE_TTL.as_secs();
        let payload =
            serde_json::to_vec(&json!({ "m": method, "sub": subject, "exp": expires, "d": data }))
                .map_err(|e| McpError::internal(format!("serialize request state: {e}")))?;
        if payload.len() > MAX_STATE_BYTES {
            return Err(McpError::invalid_params(format!(
                "request state exceeds the {MAX_STATE_BYTES}-byte limit"
            )));
        }
        let mut mac = self.mac();
        mac.update(&payload);
        let tag = mac.finalize().into_bytes();
        Ok(format!(
            "v1.{}.{}",
            URL_SAFE_NO_PAD.encode(&payload),
            URL_SAFE_NO_PAD.encode(tag)
        ))
    }

    /// Verify an inbound `requestState` and return the embedded handler data.
    ///
    /// The error is deliberately uniform — a forger learns nothing about
    /// *which* check failed. The MAC comparison is constant-time
    /// ([`Mac::verify_slice`]).
    pub(crate) fn verify(
        &self,
        method: &str,
        subject: Option<&str>,
        token: &str,
    ) -> McpResult<Value> {
        fn rejected() -> McpError {
            McpError::invalid_params("requestState failed verification")
        }
        // Bound work before touching anything attacker-sized.
        if token.len() > 2 * MAX_STATE_BYTES {
            return Err(rejected());
        }
        let mut parts = token.splitn(3, '.');
        let (Some("v1"), Some(payload), Some(tag)) = (parts.next(), parts.next(), parts.next())
        else {
            return Err(rejected());
        };
        let payload = URL_SAFE_NO_PAD.decode(payload).map_err(|_| rejected())?;
        let tag = URL_SAFE_NO_PAD.decode(tag).map_err(|_| rejected())?;
        let mut mac = self.mac();
        mac.update(&payload);
        mac.verify_slice(&tag).map_err(|_| rejected())?;

        let parsed: Value = serde_json::from_slice(&payload).map_err(|_| rejected())?;
        if parsed.get("m").and_then(Value::as_str) != Some(method) {
            return Err(rejected());
        }
        // Principal binding: the redeeming subject must match the minting one
        // (both `None` for unauthenticated requests).
        if parsed.get("sub").and_then(Value::as_str) != subject {
            return Err(rejected());
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if parsed.get("exp").and_then(Value::as_u64).unwrap_or(0) < now {
            return Err(rejected());
        }
        Ok(parsed.get("d").cloned().unwrap_or(Value::Null))
    }
}

// ---- pending server→client requests (legacy inline bidi) -----------------------

/// How long an inline bidi request waits for the client's response before the
/// handler fails with a timeout.
const BIDI_TIMEOUT: Duration = Duration::from_secs(120);

/// Routes inbound client→server *responses* back to the handler awaiting
/// them. Keys are server-minted uuid request ids, so entries are unguessable
/// and can't collide with client-issued ids; the guard removes its entry when
/// the awaiting handler finishes (or is dropped by cancellation).
#[derive(Default)]
pub(crate) struct PendingRequests {
    map: Mutex<HashMap<RequestId, oneshot::Sender<JsonRpcResponse>>>,
}

impl PendingRequests {
    fn register(
        self: &Arc<Self>,
        id: RequestId,
    ) -> (oneshot::Receiver<JsonRpcResponse>, PendingGuard) {
        let (tx, rx) = oneshot::channel();
        self.map
            .lock()
            .expect("pending map poisoned")
            .insert(id.clone(), tx);
        (
            rx,
            PendingGuard {
                pending: Arc::clone(self),
                id,
            },
        )
    }

    /// Deliver a client response to its awaiting handler. `false` if nothing
    /// was waiting (late, duplicate, or unsolicited — ignored per JSON-RPC).
    pub(crate) fn complete(&self, response: JsonRpcResponse) -> bool {
        let sender = self
            .map
            .lock()
            .expect("pending map poisoned")
            .remove(&response.id);
        match sender {
            Some(tx) => tx.send(response).is_ok(),
            None => false,
        }
    }
}

struct PendingGuard {
    pending: Arc<PendingRequests>,
    id: RequestId,
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        self.pending
            .map
            .lock()
            .expect("pending map poisoned")
            .remove(&self.id);
    }
}

// ---- the coordinator -------------------------------------------------------------

/// How this request's [`ClientHandle`] reaches the client.
enum HandleMode {
    /// Draft path: record requests, abort, answer `InputRequiredResult`.
    Mrtr,
    /// Legacy path: inline bidirectional requests over the session's
    /// server→client channel.
    Bidi {
        session: String,
        connection: String,
        pending: Arc<PendingRequests>,
    },
    /// Taskified call (SEP-2663 in-execution input): requests are published
    /// to the task (`input_required` + `inputRequests`) via the attached
    /// [`TaskInputBroker`](crate::TaskInputBroker) and the handler awaits the
    /// client's `tasks/update` answer. The slot is late-bound — the extension
    /// attaches its broker only if it actually taskifies the call; a call
    /// that ran synchronously never gets one and fails as unavailable.
    TaskMediated {
        slot: crate::extension::TaskInputSlot,
    },
    /// No client-interaction channel on this path (reason in the error).
    Unavailable(&'static str),
}

struct Inner {
    mode: HandleMode,
    /// The connection this request arrived on (empty when unknown). Used to
    /// address the initiating client for out-of-band notifications — the
    /// elicitation spec's MUST ("only ... the client that initiated").
    connection: String,
    /// The client's declared capabilities (gates which input requests may be
    /// sent — SEP-2322 MUST). `None` = nothing declared.
    client_capabilities: Option<Value>,
    /// `inputResponses` carried by this (retry) request.
    responses: BTreeMap<String, Value>,
    /// Input requests recorded by the handler this execution (key → wire
    /// request object).
    collected: Mutex<BTreeMap<String, Value>>,
    /// Verified inbound `requestState` data.
    state_in: Option<Value>,
    /// Handler-stored outbound state (signed at result assembly).
    state_out: Mutex<Option<Value>>,
    /// When set, reusing an elicit `key` with a different request shape in one
    /// execution is a hard error instead of a warning (opt-in idempotency lint).
    strict_keys: bool,
}

/// A handler's channel to the client, present only on the MRTR-capable
/// contexts (`tools/call`, `prompts/get`, `resources/read` — SEP-2322).
///
/// On the draft, `elicit` either returns the cached response from the retry
/// request or aborts the handler (via `?`) so the dispatcher can answer
/// `InputRequiredResult` — see the module docs for the re-execution contract.
/// On `2025-11-25` the same calls go out as inline bidirectional requests
/// (Phase 6f).
#[derive(Clone)]
pub struct ClientHandle {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for ClientHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientHandle").finish_non_exhaustive()
    }
}

impl ClientHandle {
    /// A handle with no client channel; every interaction fails with `reason`.
    pub(crate) fn unavailable(reason: &'static str) -> Self {
        Self {
            inner: Arc::new(Inner {
                mode: HandleMode::Unavailable(reason),
                connection: String::new(),
                client_capabilities: None,
                responses: BTreeMap::new(),
                collected: Mutex::new(BTreeMap::new()),
                state_in: None,
                state_out: Mutex::new(None),
                strict_keys: false,
            }),
        }
    }

    /// A draft-path MRTR handle for one request (re)execution.
    pub(crate) fn mrtr(
        connection: &str,
        client_capabilities: Option<Value>,
        responses: BTreeMap<String, Value>,
        state_in: Option<Value>,
        strict_keys: bool,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                mode: HandleMode::Mrtr,
                connection: connection.to_owned(),
                client_capabilities,
                responses,
                collected: Mutex::new(BTreeMap::new()),
                state_in,
                state_out: Mutex::new(None),
                strict_keys,
            }),
        }
    }

    /// A task-mediated handle for a `tools/call` offered for augmentation
    /// (SEP-2663 in-execution input). `slot` is shared with the
    /// [`CallRunner`](crate::CallRunner) so the taskifying extension can
    /// attach its broker before spawning.
    pub(crate) fn task_mediated(
        client_capabilities: Option<Value>,
        slot: crate::extension::TaskInputSlot,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                mode: HandleMode::TaskMediated { slot },
                connection: String::new(),
                client_capabilities,
                responses: BTreeMap::new(),
                collected: Mutex::new(BTreeMap::new()),
                state_in: None,
                state_out: Mutex::new(None),
                strict_keys: false,
            }),
        }
    }

    /// A legacy-path inline-bidi handle bound to one session.
    pub(crate) fn bidi(
        session: &str,
        connection: &str,
        pending: Arc<PendingRequests>,
        client_capabilities: Option<Value>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                mode: HandleMode::Bidi {
                    session: session.to_owned(),
                    connection: connection.to_owned(),
                    pending,
                },
                connection: connection.to_owned(),
                client_capabilities,
                responses: BTreeMap::new(),
                collected: Mutex::new(BTreeMap::new()),
                state_in: None,
                state_out: Mutex::new(None),
                strict_keys: false,
            }),
        }
    }

    /// Ask the user for structured input (form-mode elicitation).
    ///
    /// `key` is this elicitation's stable identity across re-executions —
    /// reuse the same key for the same question or the cached response won't
    /// be found on retry. (On the legacy inline-bidi path the key is unused
    /// on the wire but keeps handler code version-portable.)
    pub async fn elicit(
        &self,
        key: &str,
        params: neutral::ElicitParams,
    ) -> McpResult<neutral::ElicitOutcome> {
        let raw = self
            .obtain(key, "elicitation", elicit_request_value(&params))
            .await?;
        parse_elicit_outcome(&raw)
    }

    /// Ask the user to visit a URL (URL-mode elicitation, draft `mode: "url"`).
    ///
    /// The client presents `params.message` and directs the user to `params.url`
    /// (e.g. an OAuth consent page); the returned
    /// [`ElicitOutcome`](neutral::ElicitOutcome) carries the
    /// user's [`ElicitAction`](neutral::ElicitAction) with no form content. Uses
    /// the same `key` retry semantics as [`elicit`](Self::elicit).
    pub async fn elicit_url(
        &self,
        key: &str,
        params: neutral::ElicitUrlParams,
    ) -> McpResult<neutral::ElicitOutcome> {
        // Both wires require `elicitationId` on URL-mode requests (the draft
        // briefly dropped it; the 2026-07-28 RC restored it as required,
        // pairing it with `notifications/elicitation/complete`). Mint one if
        // the handler didn't set it.
        let elicitation_id = params
            .elicitation_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let raw = self
            .obtain(
                key,
                "elicitation",
                elicit_url_request_value(&params, elicitation_id),
            )
            .await?;
        parse_elicit_outcome(&raw)
    }

    /// Tell the client that the out-of-band interaction started by a URL-mode
    /// [`elicit_url`](Self::elicit_url) finished
    /// (`notifications/elicitation/complete`), so it can retry the request or
    /// update its UI without waiting on the user.
    ///
    /// Optional by spec (a MAY), and delivered only to the client that
    /// initiated the elicitation — `elicitation_id` must be the id that
    /// request carried, so set one explicitly with
    /// [`ElicitUrlParams::with_elicitation_id`](neutral::ElicitUrlParams::with_elicitation_id)
    /// when you intend to notify (an id minted for you is never surfaced).
    ///
    /// Best-effort: `false` if the initiating connection is already gone (the
    /// client's own retry controls cover that case — the spec requires them).
    pub async fn notify_elicitation_complete(&self, elicitation_id: &str) -> bool {
        let Some(writer) = request_writer(&self.inner.connection, self.session_id()) else {
            return false;
        };
        let note = turbomcp_core::JsonRpcNotification::new(
            turbomcp_protocol::methods::notification::ELICITATION_COMPLETE,
            Some(json!({ "elicitationId": elicitation_id })),
        );
        writer.send(note.into()).await.is_ok()
    }

    /// The legacy session this handle is bound to, if any (draft handles are
    /// sessionless — the draft forbids delivering request-scoped messages on
    /// any stream but the request's own).
    fn session_id(&self) -> &str {
        match &self.inner.mode {
            HandleMode::Bidi { session, .. } => session,
            _ => "",
        }
    }

    /// Ask for several inputs in **one** round trip (PLAN MR-4): all missing
    /// requests are packaged into a single `InputRequiredResult` instead of
    /// one abort per `elicit` call. Outcomes are returned in request order.
    /// (On the legacy inline-bidi path this degrades to sequential requests.)
    pub async fn elicit_all(
        &self,
        requests: Vec<(&str, neutral::ElicitParams)>,
    ) -> McpResult<Vec<neutral::ElicitOutcome>> {
        self.require_capability("elicitation")?;
        if matches!(
            self.inner.mode,
            HandleMode::Bidi { .. } | HandleMode::TaskMediated { .. }
        ) {
            // Both delivery modes resolve each request individually (no
            // batched abort), so run them in order.
            let mut outcomes = Vec::with_capacity(requests.len());
            for (key, params) in requests {
                outcomes.push(self.elicit(key, params).await?);
            }
            return Ok(outcomes);
        }
        if requests
            .iter()
            .all(|(key, _)| self.inner.responses.contains_key(*key))
        {
            return requests
                .iter()
                .map(|(key, _)| parse_elicit_outcome(&self.inner.responses[*key]))
                .collect();
        }
        for (key, params) in &requests {
            if !self.inner.responses.contains_key(*key) {
                self.record(key, elicit_request_value(params))?;
            }
        }
        Err(McpError::InputRequired)
    }

    /// Ask the client to sample its LLM (`sampling/createMessage`).
    ///
    /// Params/result are raw wire values; typed bindings come with the client
    /// work (Phase 9). Functional in both protocol versions despite the
    /// upstream deprecation marking (AUDIT F10).
    #[deprecated(note = "marked deprecated upstream; still functional in both versions")]
    pub async fn create_message(&self, key: &str, params: Value) -> McpResult<Value> {
        self.request_raw(key, request::SAMPLING_CREATE_MESSAGE, "sampling", params)
            .await
    }

    /// Ask the client for its filesystem roots (`roots/list`).
    #[deprecated(note = "marked deprecated upstream; still functional in both versions")]
    pub async fn list_roots(&self, key: &str) -> McpResult<Value> {
        self.request_raw(key, request::ROOTS_LIST, "roots", json!({}))
            .await
    }

    /// Stash typed resume state for the retry execution (PLAN MR-6). It is
    /// signed into the result's `requestState`; the retry's verified copy is
    /// readable via [`ClientHandle::load_state`].
    pub fn store_state<T: Serialize>(&self, value: &T) -> McpResult<()> {
        let value = serde_json::to_value(value)
            .map_err(|e| McpError::internal(format!("serialize state: {e}")))?;
        *self.inner.state_out.lock().expect("state lock poisoned") = Some(value);
        Ok(())
    }

    /// The verified `requestState` data from the retry request, if any.
    pub fn load_state<T: DeserializeOwned>(&self) -> McpResult<Option<T>> {
        match &self.inner.state_in {
            None | Some(Value::Null) => Ok(None),
            Some(v) => serde_json::from_value(v.clone())
                .map(Some)
                .map_err(|e| McpError::invalid_params(format!("request state shape: {e}"))),
        }
    }

    // ---- internals ---------------------------------------------------------

    fn require_capability(&self, capability: &str) -> McpResult<()> {
        if let HandleMode::Unavailable(reason) = self.inner.mode {
            return Err(McpError::internal(reason));
        }
        let declared = self
            .inner
            .client_capabilities
            .as_ref()
            .is_some_and(|caps| caps.get(capability).is_some());
        if declared {
            Ok(())
        } else {
            // SEP-2322 MUST NOT send input requests the client didn't declare.
            Err(McpError::invalid_params(format!(
                "client did not declare the `{capability}` capability"
            )))
        }
    }

    async fn request_raw(
        &self,
        key: &str,
        method: &str,
        capability: &str,
        params: Value,
    ) -> McpResult<Value> {
        self.obtain(
            key,
            capability,
            json!({ "method": method, "params": params }),
        )
        .await
    }

    /// Get the client's answer for one input request, by whichever delivery
    /// the mode prescribes: cached-response-or-abort (MRTR) or a blocking
    /// inline request (bidi).
    async fn obtain(&self, key: &str, capability: &str, request: Value) -> McpResult<Value> {
        self.require_capability(capability)?;
        match &self.inner.mode {
            HandleMode::Mrtr => {
                if let Some(raw) = self.inner.responses.get(key) {
                    return Ok(raw.clone());
                }
                self.record(key, request)?;
                Err(McpError::InputRequired)
            }
            HandleMode::Bidi {
                session,
                connection,
                pending,
            } => send_and_await(session, connection, pending, request).await,
            // Taskified call: publish to the task and await `tasks/update`.
            HandleMode::TaskMediated { slot } => match slot.get() {
                Some(broker) => broker.obtain(key, request).await,
                None => Err(McpError::internal(
                    "client input is unavailable: the call was offered for task \
                     augmentation but no input broker was attached",
                )),
            },
            // `require_capability` already rejected this mode.
            HandleMode::Unavailable(reason) => Err(McpError::internal(*reason)),
        }
    }

    /// Record an input request under `key`. Reusing a key with a different
    /// request shape in one execution is a warning by default, or a hard error
    /// when strict keys are enabled (PLAN §4.5.2 item 4).
    fn record(&self, key: &str, request: Value) -> McpResult<()> {
        let mut collected = self
            .inner
            .collected
            .lock()
            .expect("collected lock poisoned");
        if let Some(previous) = collected.get(key)
            && previous != &request
        {
            if self.inner.strict_keys {
                return Err(McpError::invalid_params(format!(
                    "elicit key `{key}` re-used with a different request shape"
                )));
            }
            tracing::warn!(key, "elicit key re-used with a different request shape");
        }
        collected.insert(key.to_owned(), request);
        Ok(())
    }

    /// The recorded input requests (dispatcher: `InputRequiredResult` assembly).
    pub(crate) fn collected(&self) -> BTreeMap<String, Value> {
        self.inner
            .collected
            .lock()
            .expect("collected lock poisoned")
            .clone()
    }

    /// The handler's outbound state, if it stored any.
    pub(crate) fn state_out(&self) -> Option<Value> {
        self.inner
            .state_out
            .lock()
            .expect("state lock poisoned")
            .clone()
    }
}

/// Send one inline bidi request on the originating request's server→client
/// channel (the request's own stream first, then the session `GET` stream —
/// see [`request_writer`](crate::subscriptions::request_writer)) and block
/// until the client's response routes back (or [`BIDI_TIMEOUT`]).
async fn send_and_await(
    session: &str,
    connection: &str,
    pending: &Arc<PendingRequests>,
    request: Value,
) -> McpResult<Value> {
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let params = request.get("params").cloned();

    // A uuid id can't collide with client-issued ids and can't be guessed.
    let id = RequestId::from(format!("srv-{}", uuid::Uuid::new_v4()));
    let (rx, _guard) = pending.register(id.clone());

    let writer = request_writer(connection, session).ok_or_else(|| {
        McpError::transport(
            "no server→client channel for this session (open the GET stream or keep the pipe alive)",
        )
    })?;
    writer
        .send(JsonRpcRequest::new(id, method, params).into())
        .await
        .map_err(|_| McpError::transport("server→client channel closed"))?;

    let response = tokio::time::timeout(BIDI_TIMEOUT, rx)
        .await
        .map_err(|_| McpError::timeout("client did not answer the input request in time"))?
        .map_err(|_| McpError::transport("server→client request dropped"))?;
    match (response.result, response.error) {
        (Some(result), None) => Ok(result),
        (_, Some(e)) => Err(McpError::internal(format!(
            "client answered input request with error {}: {}",
            e.code, e.message
        ))),
        _ => Err(McpError::internal(
            "client answered input request with an empty response",
        )),
    }
}

/// The wire `InputRequest` object for a form-mode elicitation.
fn elicit_request_value(params: &neutral::ElicitParams) -> Value {
    json!({
        "method": request::ELICITATION_CREATE,
        "params": {
            "mode": "form",
            "message": params.message,
            "requestedSchema": params.requested_schema,
        },
    })
}

/// The wire `InputRequest` object for a URL-mode elicitation. Both wires
/// require `elicitationId` (2026-07-28 RC).
fn elicit_url_request_value(params: &neutral::ElicitUrlParams, elicitation_id: String) -> Value {
    json!({
        "method": request::ELICITATION_CREATE,
        "params": {
            "mode": "url",
            "message": params.message,
            "url": params.url,
            "elicitationId": elicitation_id,
        },
    })
}

#[derive(serde::Deserialize)]
struct RawElicitResult {
    action: String,
    #[serde(default)]
    content: Map<String, Value>,
}

fn parse_elicit_outcome(raw: &Value) -> McpResult<neutral::ElicitOutcome> {
    let parsed: RawElicitResult = serde_json::from_value(raw.clone())
        .map_err(|e| McpError::invalid_params(format!("invalid elicit response: {e}")))?;
    let action = match parsed.action.as_str() {
        "accept" => neutral::ElicitAction::Accept,
        "decline" => neutral::ElicitAction::Decline,
        "cancel" => neutral::ElicitAction::Cancel,
        other => {
            return Err(McpError::invalid_params(format!(
                "invalid elicit action: {other}"
            )));
        }
    };
    let content = if action == neutral::ElicitAction::Accept {
        parsed.content
    } else {
        Map::new()
    };
    Ok(neutral::ElicitOutcome::new(action, content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_verify_roundtrip_binds_method_and_rejects_tampering() {
        let signer = StateSigner::new();
        let token = signer
            .sign("tools/call", None, &json!({"step": 2}))
            .unwrap();
        assert_eq!(
            signer.verify("tools/call", None, &token).unwrap(),
            json!({"step": 2})
        );
        // Bound to the originating method.
        assert!(signer.verify("prompts/get", None, &token).is_err());
        // A flipped byte fails the MAC.
        let mut tampered = token.clone().into_bytes();
        let mid = tampered.len() / 2;
        tampered[mid] = if tampered[mid] == b'A' { b'B' } else { b'A' };
        assert!(
            signer
                .verify("tools/call", None, &String::from_utf8(tampered).unwrap())
                .is_err()
        );
        // A different server's signer rejects it too.
        assert!(
            StateSigner::new()
                .verify("tools/call", None, &token)
                .is_err()
        );
    }

    /// The point of `ServerBuilder::with_state_key`: replicas that share a key
    /// redeem each other's states, so an elicitation survives being re-issued
    /// to a different instance (or to the same one after a restart). Without
    /// it every replica mints its own secret and MRTR breaks behind a load
    /// balancer.
    #[test]
    fn a_shared_key_lets_another_replica_redeem_the_state() {
        let key = [7u8; 32];
        let replica_a = StateSigner::from_key(key);
        let replica_b = StateSigner::from_key(key);

        let token = replica_a
            .sign("tools/call", Some("user-1"), &json!({"step": 2}))
            .unwrap();
        assert_eq!(
            replica_b
                .verify("tools/call", Some("user-1"), &token)
                .unwrap(),
            json!({"step": 2}),
            "a replica sharing the key must redeem the state"
        );

        // Sharing the key does not weaken the other bindings: a different
        // principal still cannot replay another's state.
        assert!(
            replica_b
                .verify("tools/call", Some("user-2"), &token)
                .is_err()
        );
        // And a replica on a *different* key (mid-rotation) rejects it.
        assert!(
            StateSigner::from_key([8u8; 32])
                .verify("tools/call", Some("user-1"), &token)
                .is_err()
        );
    }

    #[test]
    fn state_is_bound_to_the_minting_principal() {
        let signer = StateSigner::new();
        let token = signer
            .sign("tools/call", Some("alice"), &json!({"step": 1}))
            .unwrap();
        // Same principal redeems it.
        assert!(signer.verify("tools/call", Some("alice"), &token).is_ok());
        // A different principal — even authenticated — cannot.
        assert!(
            signer
                .verify("tools/call", Some("mallory"), &token)
                .is_err()
        );
        // Nor can an unauthenticated retry of an authenticated state.
        assert!(signer.verify("tools/call", None, &token).is_err());
    }

    #[test]
    fn oversized_state_is_rejected_at_sign_time() {
        let signer = StateSigner::new();
        let big = json!({ "blob": "x".repeat(MAX_STATE_BYTES) });
        assert!(matches!(
            signer.sign("tools/call", None, &big),
            Err(McpError::InvalidParams(_))
        ));
    }

    #[tokio::test]
    async fn elicit_without_declared_capability_is_an_error_not_an_abort() {
        let handle = ClientHandle::mrtr("", Some(json!({})), BTreeMap::new(), None, false);
        let err = handle
            .elicit("k", neutral::ElicitParams::new("?", json!({})))
            .await
            .expect_err("must not send undeclared input requests");
        assert!(matches!(err, McpError::InvalidParams(_)));
        assert!(handle.collected().is_empty(), "nothing may be recorded");
    }

    #[tokio::test]
    async fn elicit_url_records_url_mode_request() {
        let handle = ClientHandle::mrtr(
            "",
            Some(json!({ "elicitation": {} })),
            BTreeMap::new(),
            None,
            false,
        );
        let err = handle
            .elicit_url(
                "k",
                neutral::ElicitUrlParams::new("Sign in", "https://auth.example/go")
                    .with_elicitation_id("eid-1"),
            )
            .await
            .expect_err("no cached response → abort");
        assert!(matches!(err, McpError::InputRequired));
        let collected = handle.collected();
        let params = &collected["k"]["params"];
        assert_eq!(params["mode"], "url");
        assert_eq!(params["url"], "https://auth.example/go");
        // Both wires require `elicitationId` (2026-07-28 RC) — the handler's
        // explicit id is carried verbatim.
        assert_eq!(params["elicitationId"], "eid-1");
    }

    #[test]
    fn elicit_url_wire_value_carries_elicitation_id() {
        let params = neutral::ElicitUrlParams::new("Sign in", "https://auth.example/go");
        let value = elicit_url_request_value(&params, "eid-9".into());
        assert_eq!(value["params"]["elicitationId"], "eid-9");
    }

    #[tokio::test]
    async fn elicit_url_mints_an_id_when_unset() {
        let handle = ClientHandle::mrtr(
            "",
            Some(json!({ "elicitation": {} })),
            BTreeMap::new(),
            None,
            false,
        );
        let err = handle
            .elicit_url(
                "k",
                neutral::ElicitUrlParams::new("Sign in", "https://auth.example/go"),
            )
            .await
            .expect_err("no cached response → abort");
        assert!(matches!(err, McpError::InputRequired));
        let collected = handle.collected();
        let id = collected["k"]["params"]["elicitationId"]
            .as_str()
            .expect("a minted elicitationId");
        assert!(!id.is_empty());
    }

    #[tokio::test]
    async fn elicitation_complete_reaches_only_the_initiating_connection() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        let _guard = turbomcp_service::outbound::register("elicit-conn", tx);
        let handle = ClientHandle::mrtr("elicit-conn", None, BTreeMap::new(), None, false);

        assert!(handle.notify_elicitation_complete("eid-1").await);
        let turbomcp_core::JsonRpcMessage::Notification(n) = rx.try_recv().expect("a notification")
        else {
            panic!("expected a notification")
        };
        assert_eq!(n.method, "notifications/elicitation/complete");
        assert_eq!(n.params.unwrap()["elicitationId"], "eid-1");

        // No connection (an MRTR handle whose transport never named one) is a
        // no-op, not an error: the notification is a spec MAY.
        let orphan = ClientHandle::mrtr("", None, BTreeMap::new(), None, false);
        assert!(!orphan.notify_elicitation_complete("eid-1").await);
    }

    #[tokio::test]
    async fn strict_keys_reject_shape_conflict() {
        let handle = ClientHandle::mrtr(
            "",
            Some(json!({ "elicitation": {} })),
            BTreeMap::new(),
            None,
            true,
        );
        // First records under `k` and aborts (InputRequired).
        let _ = handle
            .elicit(
                "k",
                neutral::ElicitParams::new("A", json!({ "type": "object" })),
            )
            .await;
        // Same key, different request shape → strict error (not a warning).
        let err = handle
            .elicit(
                "k",
                neutral::ElicitParams::new("B", json!({ "type": "object", "extra": true })),
            )
            .await
            .expect_err("strict keys reject a shape conflict");
        assert!(matches!(err, McpError::InvalidParams(_)));
    }

    // ---- requestState verification edges ------------------------------------

    /// A token with a *valid* MAC over `payload`. Lets a test reach the checks
    /// that run after the MAC (expiry, shape), which `sign` can't be made to
    /// violate.
    fn crafted(signer: &StateSigner, payload: &Value) -> String {
        let bytes = serde_json::to_vec(payload).expect("serializable");
        let mut mac = signer.mac();
        mac.update(&bytes);
        format!(
            "v1.{}.{}",
            URL_SAFE_NO_PAD.encode(&bytes),
            URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
        )
    }

    #[track_caller]
    fn assert_uniform_rejection(result: McpResult<Value>, what: &str) {
        match result {
            Err(McpError::InvalidParams(m)) => assert_eq!(
                m, "requestState failed verification",
                "{what}: the message must not reveal which check failed"
            ),
            Err(other) => panic!("{what}: expected InvalidParams, got {other:?}"),
            Ok(v) => panic!("{what}: accepted a bad token, yielding {v}"),
        }
    }

    /// Every malformed shape must fail with the *same* message.
    ///
    /// `requestState` round-trips through the client, so it is attacker-
    /// controlled. A forger who can tell "bad base64" from "bad MAC" from
    /// "expired" learns which part to keep working on; one uniform rejection
    /// tells them nothing. This also pins the length bound, which exists so a
    /// huge token is discarded before it is decoded.
    #[test]
    fn malformed_state_tokens_are_rejected_uniformly() {
        let signer = StateSigner::new();
        let good = signer.sign("tools/call", None, &json!({ "a": 1 })).unwrap();
        let mut parts = good.splitn(3, '.');
        let (_v, payload, tag) = (
            parts.next().unwrap(),
            parts.next().unwrap().to_owned(),
            parts.next().unwrap().to_owned(),
        );

        for (what, token) in [
            ("empty", String::new()),
            ("no version prefix", format!("{payload}.{tag}")),
            ("unknown version", format!("v2.{payload}.{tag}")),
            ("only two segments", format!("v1.{payload}")),
            ("payload is not base64", format!("v1.~~~~.{tag}")),
            ("tag is not base64", format!("v1.{payload}.~~~~")),
            (
                "over the length bound",
                format!("v1.{}.{tag}", "A".repeat(2 * MAX_STATE_BYTES)),
            ),
            (
                "valid MAC over a non-JSON payload",
                crafted_bytes(&signer, b"not json at all"),
            ),
        ] {
            assert_uniform_rejection(signer.verify("tools/call", None, &token), what);
        }
    }

    /// As [`crafted`], for a payload that isn't valid JSON.
    fn crafted_bytes(signer: &StateSigner, bytes: &[u8]) -> String {
        let mut mac = signer.mac();
        mac.update(bytes);
        format!(
            "v1.{}.{}",
            URL_SAFE_NO_PAD.encode(bytes),
            URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
        )
    }

    /// The TTL is the replay bound (the mrtr spec's SHOULD). It is the one
    /// check `sign` cannot be coaxed into violating, so it needs a crafted
    /// token — and the far-future control rules out "the crafted token was
    /// simply malformed": identical construction, opposite verdict.
    #[test]
    fn an_expired_state_is_rejected_and_a_live_one_is_not() {
        let signer = StateSigner::new();
        let payload =
            |exp: u64| json!({ "m": "tools/call", "sub": null, "exp": exp, "d": { "n": 7 } });

        assert_uniform_rejection(
            signer.verify("tools/call", None, &crafted(&signer, &payload(1))),
            "expired in 1970",
        );
        assert_eq!(
            signer
                .verify("tools/call", None, &crafted(&signer, &payload(u64::MAX)))
                .expect("an unexpired crafted token verifies"),
            json!({ "n": 7 }),
            "the control proves the rejection above was the expiry, not the shape"
        );
        // A payload with no `exp` at all is treated as expired, not as
        // "unbounded" — the absent field must not become a forever token.
        assert_uniform_rejection(
            signer.verify(
                "tools/call",
                None,
                &crafted(&signer, &json!({ "m": "tools/call", "sub": null, "d": {} })),
            ),
            "no exp field",
        );
    }

    // ---- pending server→client requests --------------------------------------

    /// Responses that nothing is waiting for are dropped, per JSON-RPC. The
    /// interesting case is the *second* delivery for one id: the entry is
    /// removed on the first, so a duplicate (or a replayed) response can't
    /// resolve a later, unrelated wait that reused the id.
    #[tokio::test]
    async fn pending_requests_deliver_once_and_ignore_the_rest() {
        let pending = Arc::new(PendingRequests::default());
        let id = RequestId::from("srv-1");

        assert!(
            !pending.complete(JsonRpcResponse::success(id.clone(), json!({}))),
            "nothing registered → dropped"
        );

        let (rx, guard) = pending.register(id.clone());
        assert!(pending.complete(JsonRpcResponse::success(id.clone(), json!({ "ok": true }))));
        assert_eq!(rx.await.unwrap().result, Some(json!({ "ok": true })));
        assert!(
            !pending.complete(JsonRpcResponse::success(id.clone(), json!({}))),
            "the entry is consumed by the first delivery"
        );
        drop(guard);

        // The guard's job: a handler that goes away (cancelled, timed out)
        // must not leave its slot behind for a late response to land in.
        let (_rx, guard) = pending.register(id.clone());
        drop(guard);
        assert!(
            !pending.complete(JsonRpcResponse::success(id, json!({}))),
            "dropping the guard unregisters the wait"
        );
    }

    // ---- inline bidi (the 2025-11-25 path) -----------------------------------

    /// A bidi handle on a fresh outbound connection, plus the channel a fake
    /// client reads the server's request from.
    fn bidi_handle(
        connection: &str,
    ) -> (
        ClientHandle,
        Arc<PendingRequests>,
        tokio::sync::mpsc::Receiver<turbomcp_core::JsonRpcMessage>,
        turbomcp_service::outbound::WriterGuard,
    ) {
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let guard = turbomcp_service::outbound::register(connection, tx);
        let pending = Arc::new(PendingRequests::default());
        let handle = ClientHandle::bidi(
            "sess",
            connection,
            Arc::clone(&pending),
            Some(json!({ "elicitation": {}, "sampling": {}, "roots": {} })),
        );
        (handle, pending, rx, guard)
    }

    /// Pull the one server→client request off `rx`.
    async fn next_request(
        rx: &mut tokio::sync::mpsc::Receiver<turbomcp_core::JsonRpcMessage>,
    ) -> JsonRpcRequest {
        match rx.recv().await.expect("a server→client request") {
            turbomcp_core::JsonRpcMessage::Request(r) => r,
            other => panic!("expected a request, got {other:?}"),
        }
    }

    /// The client answering with a JSON-RPC *error* must surface as a handler
    /// error that names the code and message — an operator reading the tool's
    /// failure needs to know the client refused, and why.
    #[tokio::test]
    async fn a_client_error_answer_reaches_the_handler_with_code_and_message() {
        let (handle, pending, mut rx, _guard) = bidi_handle("bidi-err");
        let task = tokio::spawn(async move {
            handle
                .elicit("k", neutral::ElicitParams::new("?", json!({})))
                .await
        });

        let req = next_request(&mut rx).await;
        pending.complete(JsonRpcResponse::error(
            req.id,
            turbomcp_core::JsonRpcError {
                code: -32601,
                message: "elicitation unsupported".into(),
                data: None,
            },
        ));

        let err = task.await.unwrap().expect_err("the client refused");
        let msg = err.to_string();
        assert!(msg.contains("-32601"), "no code in: {msg}");
        assert!(
            msg.contains("elicitation unsupported"),
            "no reason in: {msg}"
        );
    }

    /// A frame with neither `result` nor `error` is malformed but wire-legal
    /// to *parse* (both fields default to `None`), so the handler must get a
    /// clean error rather than hanging until the 2-minute timeout.
    #[tokio::test]
    async fn an_empty_client_answer_is_an_error_not_a_hang() {
        let (handle, pending, mut rx, _guard) = bidi_handle("bidi-empty");
        let task = tokio::spawn(async move {
            handle
                .elicit("k", neutral::ElicitParams::new("?", json!({})))
                .await
        });

        let req = next_request(&mut rx).await;
        pending.complete(JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: req.id,
            result: None,
            error: None,
        });

        let err = task.await.unwrap().expect_err("neither result nor error");
        assert!(matches!(err, McpError::Internal(ref m) if m.contains("empty response")));
    }

    /// No server→client channel (the GET stream was never opened, or the pipe
    /// died) is a transport error naming the fix, not a silent stall.
    #[tokio::test]
    async fn an_elicit_with_no_server_to_client_channel_fails_fast() {
        let pending = Arc::new(PendingRequests::default());
        let handle = ClientHandle::bidi(
            "",
            "never-registered",
            pending,
            Some(json!({ "elicitation": {} })),
        );
        let err = handle
            .elicit("k", neutral::ElicitParams::new("?", json!({})))
            .await
            .expect_err("nothing to write to");
        assert!(
            matches!(err, McpError::Transport(ref m) if m.contains("GET stream")),
            "{err:?}"
        );
    }

    /// The handler must not block forever on a client that accepts the request
    /// and then never answers.
    #[tokio::test(start_paused = true)]
    async fn an_unanswered_inline_request_times_out() {
        let (handle, _pending, mut rx, _guard) = bidi_handle("bidi-timeout");
        let task = tokio::spawn(async move {
            handle
                .elicit("k", neutral::ElicitParams::new("?", json!({})))
                .await
        });
        let _req = next_request(&mut rx).await;
        // Paused time auto-advances once nothing is runnable, so this is
        // instant rather than BIDI_TIMEOUT of wall clock.
        let err = task.await.unwrap().expect_err("the client never answered");
        assert!(matches!(err, McpError::Timeout(_)), "{err:?}");
    }

    /// `elicit_all` has no batched form on the inline path — there is no
    /// abort to batch — so it degrades to one request at a time, in order.
    #[tokio::test]
    async fn elicit_all_degrades_to_sequential_requests_on_the_inline_path() {
        let (handle, pending, mut rx, _guard) = bidi_handle("bidi-all");
        let task = tokio::spawn(async move {
            handle
                .elicit_all(vec![
                    ("first", neutral::ElicitParams::new("A", json!({}))),
                    ("second", neutral::ElicitParams::new("B", json!({}))),
                ])
                .await
        });

        let first = next_request(&mut rx).await;
        assert_eq!(first.params.as_ref().unwrap()["message"], "A");
        assert!(
            rx.try_recv().is_err(),
            "the second request must wait for the first to be answered"
        );
        pending.complete(JsonRpcResponse::success(
            first.id,
            json!({ "action": "accept", "content": { "n": 1 } }),
        ));

        let second = next_request(&mut rx).await;
        assert_eq!(second.params.as_ref().unwrap()["message"], "B");
        pending.complete(JsonRpcResponse::success(
            second.id,
            json!({ "action": "decline" }),
        ));

        let outcomes = task.await.unwrap().expect("both answered");
        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0].content["n"], 1);
        assert_eq!(outcomes[1].action, neutral::ElicitAction::Decline);
    }

    // ---- MRTR batching -------------------------------------------------------

    /// The point of `elicit_all` (PLAN MR-4): every missing input is recorded
    /// in **one** abort, so the client makes one round trip instead of N.
    #[tokio::test]
    async fn elicit_all_records_every_missing_request_in_one_abort() {
        let handle = ClientHandle::mrtr(
            "",
            Some(json!({ "elicitation": {} })),
            BTreeMap::from([("first".to_owned(), json!({ "action": "accept" }))]),
            None,
            false,
        );
        let err = handle
            .elicit_all(vec![
                ("first", neutral::ElicitParams::new("A", json!({}))),
                ("second", neutral::ElicitParams::new("B", json!({}))),
                ("third", neutral::ElicitParams::new("C", json!({}))),
            ])
            .await
            .expect_err("two of three are missing");
        assert!(matches!(err, McpError::InputRequired));

        let collected = handle.collected();
        assert_eq!(
            collected.keys().collect::<Vec<_>>(),
            ["second", "third"],
            "an already-answered key must not be asked again: {collected:?}"
        );
    }

    /// Once every answer is present the retry resolves inline — no second
    /// abort, which is what makes re-execution terminate.
    #[tokio::test]
    async fn elicit_all_returns_inline_once_every_answer_is_present() {
        let handle = ClientHandle::mrtr(
            "",
            Some(json!({ "elicitation": {} })),
            BTreeMap::from([
                (
                    "a".to_owned(),
                    json!({ "action": "accept", "content": { "n": 1 } }),
                ),
                ("b".to_owned(), json!({ "action": "cancel" })),
            ]),
            None,
            false,
        );
        let outcomes = handle
            .elicit_all(vec![
                ("a", neutral::ElicitParams::new("A", json!({}))),
                ("b", neutral::ElicitParams::new("B", json!({}))),
            ])
            .await
            .expect("all cached");
        assert_eq!(outcomes[0].content["n"], 1);
        assert_eq!(outcomes[1].action, neutral::ElicitAction::Cancel);
        assert!(
            handle.collected().is_empty(),
            "a fully-answered batch records nothing"
        );
    }

    /// URL-mode elicitation resolves from the retry's cached response too —
    /// the path a real OAuth consent round trip returns on.
    #[tokio::test]
    async fn elicit_url_resolves_from_the_retry_response() {
        let handle = ClientHandle::mrtr(
            "",
            Some(json!({ "elicitation": {} })),
            BTreeMap::from([("k".to_owned(), json!({ "action": "accept" }))]),
            None,
            false,
        );
        let outcome = handle
            .elicit_url(
                "k",
                neutral::ElicitUrlParams::new("Sign in", "https://auth.example/go"),
            )
            .await
            .expect("the cached answer resolves it");
        assert!(outcome.accepted());
    }

    // ---- sampling / roots ----------------------------------------------------

    /// Both are gated on the client's declared capability and both record the
    /// spec's method name — a typo here is a request no client can answer.
    #[tokio::test]
    #[allow(deprecated)] // functional in both versions; see the method docs
    async fn sampling_and_roots_record_their_spec_methods() {
        let handle = ClientHandle::mrtr(
            "",
            Some(json!({ "sampling": {}, "roots": {} })),
            BTreeMap::new(),
            None,
            false,
        );
        assert!(matches!(
            handle.create_message("s", json!({ "messages": [] })).await,
            Err(McpError::InputRequired)
        ));
        assert!(matches!(
            handle.list_roots("r").await,
            Err(McpError::InputRequired)
        ));

        let collected = handle.collected();
        assert_eq!(collected["s"]["method"], "sampling/createMessage");
        assert_eq!(collected["s"]["params"], json!({ "messages": [] }));
        assert_eq!(collected["r"]["method"], "roots/list");

        // Undeclared is refused (SEP-2322 MUST NOT), per capability.
        let bare = ClientHandle::mrtr(
            "",
            Some(json!({ "roots": {} })),
            BTreeMap::new(),
            None,
            false,
        );
        assert!(matches!(
            bare.create_message("s", json!({})).await,
            Err(McpError::InvalidParams(_))
        ));
    }

    // ---- task-mediated delivery (SEP-2663) -----------------------------------

    /// A `tools/call` offered for augmentation whose extension never attached
    /// a broker (it ran synchronously) has nowhere to put an input request.
    /// The handler must learn that, not wait on a slot nobody will fill.
    #[tokio::test]
    async fn a_task_mediated_handle_without_a_broker_reports_it() {
        let handle = ClientHandle::task_mediated(
            Some(json!({ "elicitation": {} })),
            crate::extension::TaskInputSlot::default(),
        );
        let err = handle
            .elicit("k", neutral::ElicitParams::new("?", json!({})))
            .await
            .expect_err("no broker was attached");
        assert!(
            matches!(err, McpError::Internal(ref m) if m.contains("input broker")),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn a_task_mediated_handle_delegates_to_its_broker() {
        struct Broker;
        impl crate::extension::TaskInputBroker for Broker {
            fn obtain(
                &self,
                key: &str,
                request: Value,
            ) -> futures::future::BoxFuture<'static, McpResult<Value>> {
                let key = key.to_owned();
                Box::pin(async move {
                    assert_eq!(request["method"], "elicitation/create");
                    Ok(json!({ "action": "accept", "content": { "via": key } }))
                })
            }
        }
        let slot = crate::extension::TaskInputSlot::default();
        slot.set(Arc::new(Broker) as Arc<dyn crate::extension::TaskInputBroker>)
            .ok()
            .expect("empty slot");

        let handle = ClientHandle::task_mediated(Some(json!({ "elicitation": {} })), slot);
        let outcome = handle
            .elicit("k", neutral::ElicitParams::new("?", json!({})))
            .await
            .expect("the broker answered");
        assert_eq!(outcome.content["via"], "k");

        // `elicit_all` resolves through the broker one at a time as well.
        let outcomes = handle
            .elicit_all(vec![
                ("a", neutral::ElicitParams::new("A", json!({}))),
                ("b", neutral::ElicitParams::new("B", json!({}))),
            ])
            .await
            .expect("both answered");
        assert_eq!(outcomes[0].content["via"], "a");
        assert_eq!(outcomes[1].content["via"], "b");
    }

    /// A handle built for a path with no client channel at all (e.g. stdio
    /// `tools/list`) reports the reason it was constructed with, and reports
    /// it the same way for every interaction.
    #[tokio::test]
    async fn an_unavailable_handle_reports_its_reason() {
        let handle = ClientHandle::unavailable("no client channel on this path");
        for err in [
            handle
                .elicit("k", neutral::ElicitParams::new("?", json!({})))
                .await
                .expect_err("unavailable"),
            handle
                .elicit_all(vec![("k", neutral::ElicitParams::new("?", json!({})))])
                .await
                .expect_err("unavailable"),
        ] {
            assert!(
                matches!(err, McpError::Internal(ref m) if m == "no client channel on this path"),
                "{err:?}"
            );
        }
    }

    // ---- resume state --------------------------------------------------------

    /// `store_state`/`load_state` are the typed face of `requestState`: the
    /// handler stashes a step marker before aborting and reads it back on the
    /// re-execution.
    #[test]
    fn stored_state_round_trips_and_a_shape_mismatch_is_a_param_error() {
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct Resume {
            step: u8,
            order: String,
        }

        let handle = ClientHandle::mrtr("", None, BTreeMap::new(), None, false);
        assert!(
            handle.load_state::<Resume>().unwrap().is_none(),
            "a first execution has no inbound state"
        );
        handle
            .store_state(&Resume {
                step: 2,
                order: "o-1".into(),
            })
            .unwrap();
        let out = handle.state_out().expect("stored");

        // The retry: the dispatcher hands the verified blob back.
        let retry = ClientHandle::mrtr("", None, BTreeMap::new(), Some(out), false);
        assert_eq!(
            retry.load_state::<Resume>().unwrap(),
            Some(Resume {
                step: 2,
                order: "o-1".into()
            })
        );
        // Deploying a handler whose state type changed shape must be a clean
        // param error, not a panic in the middle of a retry.
        assert!(matches!(
            retry.load_state::<Vec<u8>>(),
            Err(McpError::InvalidParams(_))
        ));
        // An explicit JSON null is "no state", the same as absent.
        let null_state = ClientHandle::mrtr("", None, BTreeMap::new(), Some(Value::Null), false);
        assert!(null_state.load_state::<Resume>().unwrap().is_none());
    }

    // ---- elicit response parsing ---------------------------------------------

    /// A client that returns content alongside a decline/cancel must not have
    /// it surface: the handler branches on `accepted()`, and content that
    /// outlived a refusal is exactly the input a handler would wrongly trust.
    #[test]
    fn a_refused_elicitation_drops_any_content() {
        for action in ["decline", "cancel"] {
            let outcome = parse_elicit_outcome(
                &json!({ "action": action, "content": { "secret": "leaked" } }),
            )
            .expect("a well-formed refusal");
            assert!(!outcome.accepted());
            assert!(
                outcome.content.is_empty(),
                "{action} must carry no content: {:?}",
                outcome.content
            );
        }
    }

    #[test]
    fn a_malformed_elicit_response_is_a_param_error() {
        for raw in [
            json!({ "action": "maybe" }),
            json!({ "action": 7 }),
            json!({ "content": {} }),
            json!("accept"),
        ] {
            assert!(
                matches!(parse_elicit_outcome(&raw), Err(McpError::InvalidParams(_))),
                "accepted a malformed response: {raw}"
            );
        }
    }

    #[tokio::test]
    async fn non_strict_keys_only_warn_on_conflict() {
        let handle = ClientHandle::mrtr(
            "",
            Some(json!({ "elicitation": {} })),
            BTreeMap::new(),
            None,
            false,
        );
        let _ = handle
            .elicit(
                "k",
                neutral::ElicitParams::new("A", json!({ "type": "object" })),
            )
            .await;
        // A conflicting reshape aborts with InputRequired (warn), not InvalidParams.
        let err = handle
            .elicit(
                "k",
                neutral::ElicitParams::new("B", json!({ "type": "object", "extra": true })),
            )
            .await
            .expect_err("still aborts");
        assert!(matches!(err, McpError::InputRequired));
    }
}
