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
//! the client, so it is attacker-controlled input (mrtr spec MUST) and
//! readable by the client unless protected: outbound state is sealed with
//! XChaCha20-Poly1305, and the sealed payload binds a digest of the
//! originating request, the authenticated principal (a state minted for one
//! subject can't be replayed by another), and an expiry; inbound state that
//! fails any check is rejected with `-32602` before the handler runs. The key
//! defaults to a per-dispatcher secret; multi-replica deployments share keys,
//! and rotate them, via
//! [`ServerBuilder::with_state_keys`](crate::ServerBuilder::with_state_keys).
//!
//! On `2025-11-25` the same handle calls go out as **inline bidirectional
//! requests**: a real `elicitation/create` (etc.) JSON-RPC request is written
//! to the session's server→client channel and the handler blocks until the
//! client's response routes back through [`PendingRequests`]. No re-execution
//! happens on this path — handlers written for MRTR re-entry work unchanged.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chacha20poly1305::aead::{Aead as _, KeyInit as _, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};
use tokio::sync::oneshot;
use turbomcp_core::{
    JsonRpcRequest, JsonRpcResponse, McpError, McpResult, ProtocolVersion, RequestId,
};
use turbomcp_protocol::methods::request;
use turbomcp_protocol::neutral;

use crate::subscriptions::Route;

/// Default cap on the serialized `requestState` payload. Every answer collected
/// so far rides the state into the next round, and one sampling answer alone
/// can run to tens of kilobytes, so the cap has to hold a whole conversation.
pub(crate) const DEFAULT_STATE_BYTES: usize = 256 * 1024;
/// Default for how long an issued `requestState` stays redeemable (replay
/// bound — the mrtr spec's SHOULD; single-use semantics, if needed, are the
/// handler's job).
pub(crate) const DEFAULT_STATE_TTL: Duration = Duration::from_secs(10 * 60);

// ---- request-state sealing -----------------------------------------------------

/// The token format's version prefix.
const STATE_VERSION: &str = "v2";
/// Bytes of key id at the front of a sealed state.
const KID_LEN: usize = 4;
/// XChaCha20's nonce length: long enough to draw at random for every state.
const NONCE_LEN: usize = 24;

/// One key a sealer can open states with.
#[derive(Clone)]
struct StateKey {
    /// The first bytes of a hash of the key, so a token names the key that
    /// sealed it without revealing it.
    kid: [u8; KID_LEN],
    cipher: XChaCha20Poly1305,
}

impl StateKey {
    fn new(key: [u8; 32]) -> Self {
        use sha2::{Digest as _, Sha256};
        let digest = Sha256::new()
            .chain_update(b"turbomcp requestState key id")
            .chain_update(key)
            .finalize();
        let mut kid = [0u8; KID_LEN];
        kid.copy_from_slice(&digest[..KID_LEN]);
        Self {
            kid,
            cipher: XChaCha20Poly1305::new(&key.into()),
        }
    }

    /// What the AEAD binds besides the plaintext: the format and the key.
    fn associated_data(&self) -> [u8; 2 + KID_LEN] {
        let mut aad = [0u8; 2 + KID_LEN];
        aad[..2].copy_from_slice(STATE_VERSION.as_bytes());
        aad[2..].copy_from_slice(&self.kid);
        aad
    }
}

/// Seals and opens `requestState` blobs with XChaCha20-Poly1305.
///
/// A state round-trips through the client, so it is both attacker-controlled
/// input and something the client can read. Sealing it with an AEAD keeps
/// what a handler stored (and the principal it is bound to) confidential, and
/// rejects anything altered. The token is `v2.` followed by base64url of
/// `key id ‖ nonce ‖ ciphertext`.
///
/// The key defaults to a per-dispatcher random secret, which is right for a
/// single process: nothing else can mint a state it will accept, and a restart
/// invalidates every outstanding one. A deployment running more than one
/// replica supplies shared keys instead, and rotates them without breaking
/// states in flight: see
/// [`ServerBuilder::with_state_keys`](crate::ServerBuilder::with_state_keys).
#[derive(Clone)]
pub(crate) struct StateSealer {
    /// The key states are sealed with first, then the ones still accepted.
    keys: Arc<[StateKey]>,
    limit: usize,
    ttl: Duration,
}

impl StateSealer {
    pub(crate) fn new() -> Self {
        let mut key = [0u8; 32];
        getrandom::fill(&mut key).expect("the OS random source is unavailable");
        Self::from_keys(key, [])
    }

    /// A sealer that seals with `current` and also opens states sealed with
    /// any of `previous` (keys being rotated out).
    pub(crate) fn from_keys(
        current: [u8; 32],
        previous: impl IntoIterator<Item = [u8; 32]>,
    ) -> Self {
        let keys: Vec<StateKey> = std::iter::once(current)
            .chain(previous)
            .map(StateKey::new)
            .collect();
        Self {
            keys: keys.into(),
            limit: DEFAULT_STATE_BYTES,
            ttl: DEFAULT_STATE_TTL,
        }
    }

    /// The same sealer with a different payload cap.
    pub(crate) fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit.max(1024);
        self
    }

    /// The same sealer with a different redemption window.
    pub(crate) fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    pub(crate) fn limit(&self) -> usize {
        self.limit
    }

    pub(crate) fn ttl(&self) -> Duration {
        self.ttl
    }

    /// Seal handler `data` into the opaque wire string. The sealed payload
    /// binds the originating `method` (a request binding), the authenticated
    /// `subject` (a state minted for one principal can't be replayed by
    /// another; `None` for an unauthenticated request), and an expiry.
    pub(crate) fn seal(
        &self,
        method: &str,
        subject: Option<&str>,
        data: &Value,
    ) -> McpResult<String> {
        let expires = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            + self.ttl.as_secs();
        let payload =
            serde_json::to_vec(&json!({ "m": method, "sub": subject, "exp": expires, "d": data }))
                .map_err(|e| McpError::internal(format!("serialize request state: {e}")))?;
        // The server's own configuration, not anything the client sent: an
        // `invalid_params` here told the client to fix a request it cannot
        // change, and every retry failed the same way.
        if payload.len() > self.limit {
            return Err(McpError::internal(format!(
                "request state is {} bytes, over this server's {}-byte limit; raise it with \
                 `ServerBuilder::request_state_limit`",
                payload.len(),
                self.limit
            )));
        }
        self.seal_bytes(&payload)
    }

    fn seal_bytes(&self, payload: &[u8]) -> McpResult<String> {
        let key = &self.keys[0];
        let mut nonce = [0u8; NONCE_LEN];
        getrandom::fill(&mut nonce).expect("the OS random source is unavailable");
        let sealed = key
            .cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: payload,
                    aad: &key.associated_data(),
                },
            )
            .map_err(|_| McpError::internal("could not seal request state"))?;
        let mut token = Vec::with_capacity(KID_LEN + NONCE_LEN + sealed.len());
        token.extend_from_slice(&key.kid);
        token.extend_from_slice(&nonce);
        token.extend_from_slice(&sealed);
        Ok(format!("{STATE_VERSION}.{}", URL_SAFE_NO_PAD.encode(token)))
    }

    /// Open an inbound `requestState` and return the embedded handler data.
    ///
    /// The error is deliberately uniform: a forger learns nothing about
    /// *which* check failed.
    pub(crate) fn open(
        &self,
        method: &str,
        subject: Option<&str>,
        token: &str,
    ) -> McpResult<Value> {
        fn rejected() -> McpError {
            McpError::invalid_params("requestState failed verification")
        }
        // Bound work before touching anything attacker-sized.
        if token.len() > 2 * self.limit {
            return Err(rejected());
        }
        let encoded = token
            .strip_prefix(STATE_VERSION)
            .and_then(|rest| rest.strip_prefix('.'))
            .ok_or_else(rejected)?;
        let bytes = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| rejected())?;
        if bytes.len() < KID_LEN + NONCE_LEN {
            return Err(rejected());
        }
        let (kid, rest) = bytes.split_at(KID_LEN);
        let (nonce, sealed) = rest.split_at(NONCE_LEN);
        let payload = self
            .keys
            .iter()
            .filter(|key| key.kid == kid)
            .find_map(|key| {
                key.cipher
                    .decrypt(
                        XNonce::from_slice(nonce),
                        Payload {
                            msg: sealed,
                            aad: &key.associated_data(),
                        },
                    )
                    .ok()
            })
            .ok_or_else(rejected)?;
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
        // An id-less error answers a frame the client couldn't read; there is
        // no request of ours to hand it to.
        let Some(id) = &response.id else {
            return false;
        };
        let sender = self.map.lock().expect("pending map poisoned").remove(id);
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
    Bidi { pending: Arc<PendingRequests> },
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
    /// The revision this session negotiated.
    ///
    /// [`HandleMode`] is not a substitute: `Bidi` covers `2025-11-25` *and*
    /// `2025-06-18`, which differ on what a server→client request may carry.
    /// Without this the older wire silently received `2025-11-25` shapes.
    version: ProtocolVersion,
    /// Where this request's server→client messages go: its own stream, then
    /// (legacy only) the session's `GET` stream. Also how the initiating
    /// client is addressed for out-of-band notifications, the elicitation
    /// spec's MUST ("only ... the client that initiated").
    route: Route,
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
    /// Set when this handle raised the MRTR abort. The dispatcher answers
    /// `InputRequiredResult` from this, not from the error the handler
    /// returned, which may have been wrapped along the way.
    aborted: AtomicBool,
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
                // Nothing is ever rendered on this handle; every call fails
                // before it reaches a wire.
                version: ProtocolVersion::LATEST,
                route: Route::default(),
                client_capabilities: None,
                responses: BTreeMap::new(),
                collected: Mutex::new(BTreeMap::new()),
                aborted: AtomicBool::new(false),
                state_in: None,
                state_out: Mutex::new(None),
                strict_keys: false,
            }),
        }
    }

    /// A draft-path MRTR handle for one request (re)execution.
    pub(crate) fn mrtr(
        route: Route,
        client_capabilities: Option<Value>,
        responses: BTreeMap<String, Value>,
        state_in: Option<Value>,
        strict_keys: bool,
    ) -> Self {
        // A retry carries only the answers to the round that just finished, so
        // the framework carries the earlier ones forward inside the signed
        // state. Without that, a handler asking two questions in sequence
        // would re-ask the first one on every round and never finish — the
        // client is not required to accumulate, and SEP-2322's conformance
        // scenario proves it does not.
        let (handler_state, carried) = StateEnvelope::split(state_in);
        let mut merged = carried;
        // This round's answers win: a client re-sending a key is answering
        // again, not replaying.
        merged.extend(responses);
        Self {
            inner: Arc::new(Inner {
                mode: HandleMode::Mrtr,
                // MRTR is the 2026-07-28 delivery model and no other.
                version: ProtocolVersion::V2026_07_28,
                route,
                client_capabilities,
                responses: merged,
                collected: Mutex::new(BTreeMap::new()),
                aborted: AtomicBool::new(false),
                // Resume state lives until replaced or cleared, like the
                // answers beside it. Starting each round empty dropped it the
                // first round a handler read it without storing it again, so
                // "store once, load on retry" lost its state by round three.
                state_out: Mutex::new(handler_state.clone()),
                state_in: handler_state,
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
                // Task-mediated input is the 2026-07-28 Tasks extension.
                version: ProtocolVersion::V2026_07_28,
                route: Route::default(),
                client_capabilities,
                responses: BTreeMap::new(),
                collected: Mutex::new(BTreeMap::new()),
                aborted: AtomicBool::new(false),
                state_in: None,
                state_out: Mutex::new(None),
                strict_keys: false,
            }),
        }
    }

    /// A legacy-path inline-bidi handle bound to one session, on `version`
    /// (`2025-11-25` or `2025-06-18` — the two differ in what a server→client
    /// request may carry, so the handle has to know which).
    pub(crate) fn bidi(
        route: Route,
        pending: Arc<PendingRequests>,
        client_capabilities: Option<Value>,
        version: ProtocolVersion,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                mode: HandleMode::Bidi { pending },
                version,
                route,
                client_capabilities,
                responses: BTreeMap::new(),
                collected: Mutex::new(BTreeMap::new()),
                aborted: AtomicBool::new(false),
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
        self.prepare_elicit(&params)?;
        let raw = self
            .obtain(key, self.form_capability(), elicit_request_value(&params))
            .await?;
        checked_outcome(&params, &raw)
    }

    /// Which capability a *form*-mode elicitation needs from this client.
    ///
    /// "Clients declaring the `elicitation` capability MUST support at least
    /// one mode (`form` or `url`)", so a client may legally declare
    /// `{"elicitation": {"url": {}}}` and render no forms at all — and sending
    /// it one strands the user exactly as an undeclared capability would.
    ///
    /// But `2025-06-18` has no sub-capabilities: there, bare `elicitation` has
    /// to keep meaning "I can render a form" or every client on that revision
    /// breaks. So does a `2025-11-25` client that declared `{}` and named no
    /// mode — it has said nothing to contradict. The sub-capability is
    /// required only of a client that *did* name its modes and left `form` out.
    fn form_capability(&self) -> &'static str {
        if matches!(self.inner.version, ProtocolVersion::V2025_06_18) {
            return "elicitation";
        }
        let named_modes = self
            .inner
            .client_capabilities
            .as_ref()
            .and_then(|caps| caps.get("elicitation"))
            .and_then(Value::as_object)
            .is_some_and(|modes| !modes.is_empty());
        if named_modes {
            "elicitation.form"
        } else {
            "elicitation"
        }
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
        // "The `url` parameter MUST contain a valid URL." Checked with a real
        // parser rather than a prefix test: the client is about to put this in
        // front of a user, and a relative or malformed one resolves against
        // whatever the client's UI happens to be.
        let url = fluent_uri::Uri::parse(params.url.as_str()).map_err(|e| {
            McpError::invalid_params(format!("elicitation url `{}`: {e}", params.url))
        })?;
        let scheme = url.scheme().as_str();
        if !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")) {
            return Err(McpError::invalid_params(format!(
                "elicitation url `{}` has scheme `{scheme}`; a user is being sent there, \
                 so it must be http or https",
                params.url,
            )));
        }
        if url.authority().is_none_or(|a| a.host().is_empty()) {
            return Err(McpError::invalid_params(format!(
                "elicitation url `{}` names no host",
                params.url
            )));
        }
        // `elicitationId` is `2025-11-25`-only. The 2026-07-28 RC had briefly
        // made it required on URL-mode requests; the frozen spec removed it
        // again, together with `notifications/elicitation/complete`, so the
        // draft wire must not carry it. Mint one only where it belongs, if the
        // handler didn't set it.
        let elicitation_id = self.wire_carries_elicitation_id().then(|| {
            params
                .elicitation_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
        });
        // URL mode is its own declaration: a form-only client has nowhere to
        // send the user. Form mode is gated too, but more loosely — see
        // [`form_capability`](Self::form_capability) for why bare `elicitation`
        // still means "I render forms".
        let raw = self
            .obtain(
                key,
                "elicitation.url",
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
    /// **`2025-11-25` only.** The frozen `2026-07-28` deleted this
    /// notification along with `elicitationId`, so on a draft handle this is a
    /// no-op returning `false` rather than a notification the client's schema
    /// does not define.
    ///
    /// Best-effort otherwise: `false` if the initiating connection is already
    /// gone (the client's own retry controls cover that case — the spec
    /// requires them).
    pub async fn notify_elicitation_complete(&self, elicitation_id: &str) -> bool {
        if !self.wire_carries_elicitation_id() {
            return false;
        }
        let Some(writer) = self.inner.route.peer() else {
            return false;
        };
        let note = turbomcp_core::JsonRpcNotification::new(
            turbomcp_protocol::methods::notification::ELICITATION_COMPLETE,
            Some(json!({ "elicitationId": elicitation_id })),
        );
        writer.send(note.into()).await.is_ok()
    }

    /// Whether this handle's wire defines URL-elicitation correlation:
    /// `elicitationId` on the request and the paired
    /// `notifications/elicitation/complete`.
    ///
    /// Only `2025-11-25` does. The frozen `2026-07-28` schema has neither field
    /// nor notification (the RC briefly had both) and `2025-06-18` predates
    /// URL-mode elicitation entirely, so sending either anywhere else would be
    /// inventing protocol.
    fn wire_carries_elicitation_id(&self) -> bool {
        matches!(self.inner.version, ProtocolVersion::V2025_11_25)
    }

    /// Ask for several inputs in **one** round trip (PLAN MR-4): all missing
    /// requests are packaged into a single `InputRequiredResult` instead of
    /// one abort per `elicit` call. Outcomes are returned in request order.
    /// (On the legacy inline-bidi path this degrades to sequential requests.)
    pub async fn elicit_all(
        &self,
        requests: Vec<(&str, neutral::ElicitParams)>,
    ) -> McpResult<Vec<neutral::ElicitOutcome>> {
        self.require_capability(self.form_capability())?;
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
        // The same checks `elicit` makes: on this path the requests go out
        // batched in `inputRequests`, and an unrenderable form used to reach
        // the client here while the identical call failed cleanly elsewhere.
        for (_, params) in &requests {
            self.prepare_elicit(params)?;
        }
        if requests
            .iter()
            .all(|(key, _)| self.inner.responses.contains_key(*key))
        {
            return requests
                .iter()
                .map(|(key, params)| checked_outcome(params, &self.inner.responses[*key]))
                .collect();
        }
        for (key, params) in &requests {
            if !self.inner.responses.contains_key(*key) {
                self.record(key, elicit_request_value(params))?;
            }
        }
        Err(self.abort())
    }

    /// Ask the client to sample its LLM (`sampling/createMessage`).
    ///
    /// The conversation is rendered for *this session's* revision: `2025-06-18`
    /// has no multi-block messages and no agentic sampling, so a request that
    /// needs either is refused here rather than being truncated into something
    /// the model would answer wrongly. Functional on all three revisions
    /// despite the upstream deprecation marking (AUDIT F10).
    #[deprecated(note = "marked deprecated upstream; still functional in every version")]
    pub async fn create_message(
        &self,
        key: &str,
        params: neutral::CreateMessageParams,
    ) -> McpResult<neutral::CreateMessageResult> {
        // "Servers MUST NOT send tool-enabled sampling requests to Clients
        // that have not declared support for tool use via the `sampling.tools`
        // capability" (2026-07-28 client/sampling.mdx, same on 2025-11-25).
        // `includeContext` is the same shape of promise: undeclared, the spec
        // says send only `none`. Which capability this request needs is a
        // property of the request, so it is read off the params rather than
        // fixed at the call site.
        // A request can need both; each is its own promise. The narrowest one
        // gates the delivery below, and the other is checked here first.
        if params.uses_tools() && params.uses_context() {
            self.require_capability("sampling.context")?;
        }
        let capability = if params.uses_tools() {
            "sampling.tools"
        } else if params.uses_context() {
            "sampling.context"
        } else {
            "sampling"
        };
        let wire = params.to_wire(&self.inner.version).map_err(sampling_err)?;
        let raw = self
            .request_raw(key, request::SAMPLING_CREATE_MESSAGE, capability, wire)
            .await?;
        neutral::CreateMessageResult::from_wire(&raw).map_err(|e| {
            McpError::invalid_params(format!("invalid sampling/createMessage result: {e}"))
        })
    }

    /// Ask the client for its filesystem roots (`roots/list`).
    ///
    /// Entries whose `uri` is not a `file://` URI are dropped: the spec makes
    /// that scheme a MUST, and a handler that trusted an arbitrary scheme here
    /// would be reading whatever the client named.
    #[deprecated(note = "marked deprecated upstream; still functional in every version")]
    pub async fn list_roots(&self, key: &str) -> McpResult<Vec<neutral::Root>> {
        let raw = self
            .request_raw(key, request::ROOTS_LIST, "roots", json!({}))
            .await?;
        Ok(neutral::Root::list_from_wire(&raw))
    }

    /// Stash typed resume state for every later round of this request, until
    /// it is replaced or [cleared](Self::clear_state) (PLAN MR-6). It is signed
    /// into the result's `requestState` — signed, not encrypted: the client
    /// can read it — and each retry's verified copy is readable via
    /// [`ClientHandle::load_state`].
    pub fn store_state<T: Serialize>(&self, value: &T) -> McpResult<()> {
        let value = serde_json::to_value(value)
            .map_err(|e| McpError::internal(format!("serialize state: {e}")))?;
        *self.inner.state_out.lock().expect("state lock poisoned") = Some(value);
        Ok(())
    }

    /// Drop the stored resume state, so later rounds see none.
    pub fn clear_state(&self) {
        *self.inner.state_out.lock().expect("state lock poisoned") = None;
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

    /// What every form elicitation checks before it goes out: the schema is in
    /// the form subset this session's revision can render.
    fn prepare_elicit(&self, params: &neutral::ElicitParams) -> McpResult<()> {
        params
            .validate_for(&self.inner.version)
            .map_err(McpError::invalid_params)
    }

    /// Whether the client declared `capability`, which may be a dotted path
    /// into a sub-capability (`elicitation.url`, `sampling.tools`).
    ///
    /// Testing only the top-level key was not enough. `elicitation` and
    /// `sampling` each carry sub-objects that say *which* variant the client
    /// can service — a client declaring `elicitation.form` and nothing else
    /// renders a form and cannot open a consent page — and sending the variant
    /// it did not declare strands the interaction exactly as sending an
    /// undeclared capability would.
    fn require_capability(&self, capability: &str) -> McpResult<()> {
        if let HandleMode::Unavailable(reason) = self.inner.mode {
            return Err(McpError::internal(reason));
        }
        let declared = self.inner.client_capabilities.as_ref().is_some_and(|caps| {
            capability
                .split('.')
                .try_fold(caps, |node, segment| node.get(segment))
                .is_some()
        });
        if declared {
            Ok(())
        } else {
            // SEP-2322: MUST NOT send input requests the client didn't
            // declare. This is a protocol-level refusal, not a tool failure —
            // the call was never valid to make — so it carries
            // `MissingRequiredCapability` (`-32021`) and propagates past the
            // `is_error` conversion, naming the capability so the client can
            // re-declare and retry.
            Err(McpError::MissingRequiredCapability(capability.to_owned()))
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
                Err(self.abort())
            }
            HandleMode::Bidi { pending } => {
                send_and_await(&self.inner.route, pending, request).await
            }
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

    /// Raise the MRTR abort: the sentinel to carry out through `?`, and the
    /// flag that says it happened whatever becomes of the sentinel.
    fn abort(&self) -> McpError {
        self.inner.aborted.store(true, Ordering::Release);
        McpError::InputRequired
    }

    /// Whether this handle raised the MRTR abort this execution.
    pub(crate) fn aborted(&self) -> bool {
        self.inner.aborted.load(Ordering::Acquire)
    }

    /// The recorded input requests (dispatcher: `InputRequiredResult` assembly).
    pub(crate) fn collected(&self) -> BTreeMap<String, Value> {
        self.inner
            .collected
            .lock()
            .expect("collected lock poisoned")
            .clone()
    }

    /// The state to sign into this turn's `requestState`: the handler's own
    /// value plus every answer known so far.
    ///
    /// `None` only when there is genuinely nothing to carry — the handler
    /// stored nothing and no question has been answered yet — so a first-round
    /// abort with no stored state still emits just `inputRequests`, as before.
    pub(crate) fn state_out(&self) -> Option<Value> {
        let handler = self
            .inner
            .state_out
            .lock()
            .expect("state lock poisoned")
            .clone();
        StateEnvelope::join(handler, &self.inner.responses)
    }
}

/// How MRTR state is packed into the opaque, signed `requestState`.
///
/// Two things share it: whatever the handler stored via
/// [`ClientHandle::store_state`], and the answers already collected in earlier
/// rounds. They are kept in separate slots so a handler's own state shape is
/// never disturbed by the bookkeeping, and [`ClientHandle::load_state`] still
/// sees exactly what it stored.
///
/// The blob is opaque to clients (they echo it back verbatim), so its shape is
/// not wire-visible; state minted before this envelope existed still loads,
/// since anything that isn't a tagged envelope is read as bare handler state.
struct StateEnvelope;

impl StateEnvelope {
    /// Marks an object as an envelope rather than bare handler state.
    const TAG: &'static str = "io.turbomcp/mrtr";
    const HANDLER: &'static str = "state";
    const ANSWERS: &'static str = "answers";

    /// Split verified inbound state into (handler state, carried answers).
    fn split(state_in: Option<Value>) -> (Option<Value>, BTreeMap<String, Value>) {
        let Some(value) = state_in else {
            return (None, BTreeMap::new());
        };
        let is_envelope = value
            .get(Self::TAG)
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !is_envelope {
            return (Some(value), BTreeMap::new());
        }
        let answers = value
            .get(Self::ANSWERS)
            .and_then(Value::as_object)
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();
        (value.get(Self::HANDLER).cloned(), answers)
    }

    /// Pack handler state and answers back together, or `None` if both empty.
    fn join(handler: Option<Value>, answers: &BTreeMap<String, Value>) -> Option<Value> {
        if handler.is_none() && answers.is_empty() {
            return None;
        }
        let mut envelope = serde_json::Map::new();
        envelope.insert(Self::TAG.to_owned(), Value::Bool(true));
        if let Some(handler) = handler {
            envelope.insert(Self::HANDLER.to_owned(), handler);
        }
        if !answers.is_empty() {
            envelope.insert(
                Self::ANSWERS.to_owned(),
                Value::Object(
                    answers
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                ),
            );
        }
        Some(Value::Object(envelope))
    }
}

/// Send one inline bidi request on the originating request's server→client
/// channel (the request's own stream first, then the session `GET` stream;
/// see [`Route`]) and block until the client's response routes back (or
/// [`BIDI_TIMEOUT`]).
async fn send_and_await(
    route: &Route,
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

    let writer = route.peer().ok_or_else(|| {
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

/// A conversation this session's wire cannot carry is the handler's mistake
/// rather than the client's, so it surfaces with the reason spelled out instead
/// of a truncated request the model would answer wrongly.
fn sampling_err(e: neutral::SamplingError) -> McpError {
    McpError::invalid_params(format!("sampling/createMessage: {e}"))
}

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

/// The wire `InputRequest` object for a URL-mode elicitation. `elicitation_id`
/// is `Some` only on `2025-11-25`, the one wire that defines `elicitationId`
/// (see [`ClientHandle::wire_carries_elicitation_id`]).
fn elicit_url_request_value(
    params: &neutral::ElicitUrlParams,
    elicitation_id: Option<String>,
) -> Value {
    let mut wire = json!({
        "mode": "url",
        "message": params.message,
        "url": params.url,
    });
    if let Some(id) = elicitation_id {
        wire["elicitationId"] = Value::String(id);
    }
    json!({
        "method": request::ELICITATION_CREATE,
        "params": wire,
    })
}

#[derive(serde::Deserialize)]
struct RawElicitResult {
    action: String,
    #[serde(default)]
    content: Map<String, Value>,
}

/// The client's answer to a form elicitation, checked against what was asked.
///
/// "Servers SHOULD validate received data matches the requested schema"
/// (elicitation.mdx §Form Mode Security, 2025-11-25 and 2026-07-28; 2025-06-18
/// says both parties SHOULD). An accepted form is entirely client-supplied,
/// and on MRTR it is whatever the client put in `inputResponses`; without this,
/// every handler would have to re-validate by hand, or forget to.
fn checked_outcome(
    params: &neutral::ElicitParams,
    raw: &Value,
) -> McpResult<neutral::ElicitOutcome> {
    let outcome = parse_elicit_outcome(raw)?;
    if outcome.accepted() {
        let validator = jsonschema::validator_for(&params.requested_schema)
            .map_err(|e| McpError::internal(format!("invalid requestedSchema: {e}")))?;
        let content = Value::Object(outcome.content.clone());
        if let Err(e) = validator.validate(&content) {
            return Err(McpError::invalid_params(format!(
                "the client's answer does not match the requested schema: {e}"
            )));
        }
    }
    Ok(outcome)
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

    /// A minimal schema inside the form subset, for the tests that care about
    /// delivery rather than what is being asked.
    fn form_schema() -> Value {
        json!({ "type": "object", "properties": { "answer": { "type": "string" } } })
    }

    #[test]
    fn sign_verify_roundtrip_binds_method_and_rejects_tampering() {
        let sealer = StateSealer::new();
        let token = sealer
            .seal("tools/call", None, &json!({"step": 2}))
            .unwrap();
        assert_eq!(
            sealer.open("tools/call", None, &token).unwrap(),
            json!({"step": 2})
        );
        // Bound to the originating method.
        assert!(sealer.open("prompts/get", None, &token).is_err());
        // A flipped byte fails authentication.
        let mut tampered = token.clone().into_bytes();
        let mid = tampered.len() / 2;
        tampered[mid] = if tampered[mid] == b'A' { b'B' } else { b'A' };
        assert!(
            sealer
                .open("tools/call", None, &String::from_utf8(tampered).unwrap())
                .is_err()
        );
        // A different server's sealer rejects it too.
        assert!(StateSealer::new().open("tools/call", None, &token).is_err());
    }

    /// The client can't read what a handler stored, nor the principal it is
    /// bound to: the state is sealed, not just signed.
    #[test]
    fn a_sealed_state_reveals_nothing_to_the_client() {
        let sealer = StateSealer::new();
        let token = sealer
            .seal(
                "tools/call",
                Some("alice@example.com"),
                &json!({ "secret": "the-handler-kept-this" }),
            )
            .unwrap();
        let bytes = URL_SAFE_NO_PAD
            .decode(token.strip_prefix("v2.").unwrap())
            .unwrap();
        let shown = String::from_utf8_lossy(&bytes);
        assert!(!shown.contains("the-handler-kept-this"), "{shown}");
        assert!(!shown.contains("alice"), "{shown}");
        // Two seals of the same state differ (a fresh nonce each time).
        let again = sealer
            .seal(
                "tools/call",
                Some("alice@example.com"),
                &json!({ "secret": "the-handler-kept-this" }),
            )
            .unwrap();
        assert_ne!(token, again);
    }

    /// Rotation: a key moved to `previous` still opens the states it sealed,
    /// new states are sealed with the current key, and a key that was dropped
    /// opens nothing.
    #[test]
    fn rotated_keys_keep_states_in_flight_working() {
        let old = [1u8; 32];
        let new = [2u8; 32];
        let before = StateSealer::from_keys(old, []);
        let in_flight = before.seal("tools/call", None, &json!(1)).unwrap();

        let during = StateSealer::from_keys(new, [old]);
        assert_eq!(
            during.open("tools/call", None, &in_flight).unwrap(),
            json!(1)
        );
        let fresh = during.seal("tools/call", None, &json!(2)).unwrap();
        assert!(
            before.open("tools/call", None, &fresh).is_err(),
            "sealed with the new key"
        );

        let after = StateSealer::from_keys(new, []);
        assert!(after.open("tools/call", None, &in_flight).is_err());
        assert_eq!(after.open("tools/call", None, &fresh).unwrap(), json!(2));
    }

    /// The point of `ServerBuilder::with_state_key`: replicas that share a key
    /// redeem each other's states, so an elicitation survives being re-issued
    /// to a different instance (or to the same one after a restart). Without
    /// it every replica mints its own secret and MRTR breaks behind a load
    /// balancer.
    #[test]
    fn a_shared_key_lets_another_replica_redeem_the_state() {
        let key = [7u8; 32];
        let replica_a = StateSealer::from_keys(key, []);
        let replica_b = StateSealer::from_keys(key, []);

        let token = replica_a
            .seal("tools/call", Some("user-1"), &json!({"step": 2}))
            .unwrap();
        assert_eq!(
            replica_b
                .open("tools/call", Some("user-1"), &token)
                .unwrap(),
            json!({"step": 2}),
            "a replica sharing the key must redeem the state"
        );

        // Sharing the key does not weaken the other bindings: a different
        // principal still cannot replay another's state.
        assert!(
            replica_b
                .open("tools/call", Some("user-2"), &token)
                .is_err()
        );
        // And a replica on a *different* key (mid-rotation) rejects it.
        assert!(
            StateSealer::from_keys([8u8; 32], [])
                .open("tools/call", Some("user-1"), &token)
                .is_err()
        );
    }

    #[test]
    fn state_is_bound_to_the_minting_principal() {
        let sealer = StateSealer::new();
        let token = sealer
            .seal("tools/call", Some("alice"), &json!({"step": 1}))
            .unwrap();
        // Same principal redeems it.
        assert!(sealer.open("tools/call", Some("alice"), &token).is_ok());
        // A different principal — even authenticated — cannot.
        assert!(sealer.open("tools/call", Some("mallory"), &token).is_err());
        // Nor can an unauthenticated retry of an authenticated state.
        assert!(sealer.open("tools/call", None, &token).is_err());
    }

    #[test]
    fn oversized_state_is_the_servers_error_and_names_the_knob() {
        let sealer = StateSealer::new();
        let big = json!({ "blob": "x".repeat(DEFAULT_STATE_BYTES) });
        let err = sealer.seal("tools/call", None, &big).unwrap_err();
        assert!(matches!(err, McpError::Internal(_)), "{err:?}");
        assert!(err.to_string().contains("request_state_limit"), "{err}");
        // And a larger configured limit admits it.
        let roomy = StateSealer::new().with_limit(2 * DEFAULT_STATE_BYTES);
        assert!(roomy.seal("tools/call", None, &big).is_ok());
    }

    /// An undeclared capability is `MissingRequiredCapability` (`-32021`), not
    /// invalid params: SEP-2575 requires the refusal to name what the client
    /// must declare so it can re-declare and retry, and a `-32602` carries no
    /// such affordance.
    #[tokio::test]
    async fn elicit_without_declared_capability_is_an_error_not_an_abort() {
        let handle = ClientHandle::mrtr(
            Route::default(),
            Some(json!({})),
            BTreeMap::new(),
            None,
            false,
        );
        let err = handle
            .elicit("k", neutral::ElicitParams::new("?", form_schema()))
            .await
            .expect_err("must not send undeclared input requests");
        assert!(
            matches!(&err, McpError::MissingRequiredCapability(c) if c == "elicitation"),
            "got {err:?}"
        );
        assert!(handle.collected().is_empty(), "nothing may be recorded");
    }

    #[tokio::test]
    async fn elicit_url_records_url_mode_request() {
        let handle = ClientHandle::mrtr(
            Route::default(),
            // URL mode is declared explicitly; bare `elicitation` is form.
            Some(json!({ "elicitation": { "url": {} } })),
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
        // The frozen `2026-07-28` has no `elicitationId`. Even a handler that
        // sets one explicitly must not put it on this wire — the field was
        // deleted at freeze along with `notifications/elicitation/complete`.
        assert!(
            params.get("elicitationId").is_none(),
            "the draft wire defines no elicitationId"
        );
    }

    /// The legacy counterpart: `2025-11-25` *requires* `elicitationId` on a
    /// URL-mode request, and the handler's explicit id is carried verbatim.
    #[tokio::test]
    async fn elicit_url_carries_elicitation_id_on_legacy() {
        let (handle, pending, mut rx, _guard) = bidi_handle("bidi-elicit-url");
        let task = tokio::spawn(async move {
            handle
                .elicit_url(
                    "k",
                    neutral::ElicitUrlParams::new("Sign in", "https://auth.example/go")
                        .with_elicitation_id("eid-1"),
                )
                .await
        });

        let req = next_request(&mut rx).await;
        let params = req.params.clone().expect("params");
        assert_eq!(params["mode"], "url");
        assert_eq!(params["elicitationId"], "eid-1");
        pending.complete(JsonRpcResponse::success(
            req.id,
            json!({ "action": "accept" }),
        ));
        task.await.unwrap().expect("the client accepted");
    }

    #[test]
    fn elicit_url_wire_value_carries_elicitation_id_only_when_given() {
        let params = neutral::ElicitUrlParams::new("Sign in", "https://auth.example/go");
        let legacy = elicit_url_request_value(&params, Some("eid-9".to_string()));
        assert_eq!(legacy["params"]["elicitationId"], "eid-9");
        let draft = elicit_url_request_value(&params, None);
        assert!(draft["params"].get("elicitationId").is_none());
        // The fields both wires share survive either way.
        assert_eq!(draft["params"]["mode"], "url");
        assert_eq!(draft["params"]["url"], "https://auth.example/go");
    }

    /// Minting only happens on the wire that has somewhere to put the id.
    #[tokio::test]
    async fn elicit_url_mints_an_id_when_unset_on_legacy() {
        let (handle, pending, mut rx, _guard) = bidi_handle("bidi-elicit-mint");
        let task = tokio::spawn(async move {
            handle
                .elicit_url(
                    "k",
                    neutral::ElicitUrlParams::new("Sign in", "https://auth.example/go"),
                )
                .await
        });

        let req = next_request(&mut rx).await;
        let params = req.params.clone().expect("params");
        let id = params["elicitationId"]
            .as_str()
            .expect("a minted elicitationId");
        assert!(!id.is_empty());
        pending.complete(JsonRpcResponse::success(
            req.id,
            json!({ "action": "accept" }),
        ));
        task.await.unwrap().expect("the client accepted");
    }

    #[tokio::test]
    async fn elicitation_complete_reaches_only_the_initiating_connection() {
        let (handle, _pending, mut rx, _guard) = bidi_handle("elicit-conn");

        assert!(handle.notify_elicitation_complete("eid-1").await);
        let turbomcp_core::JsonRpcMessage::Notification(n) = rx.try_recv().expect("a notification")
        else {
            panic!("expected a notification")
        };
        assert_eq!(n.method, "notifications/elicitation/complete");
        assert_eq!(n.params.unwrap()["elicitationId"], "eid-1");

        // No connection (a handle whose transport never named one) is a no-op,
        // not an error: the notification is a spec MAY.
        let orphan = ClientHandle::bidi(
            Route::default(),
            Arc::new(PendingRequests::default()),
            None,
            ProtocolVersion::V2025_11_25,
        );
        assert!(!orphan.notify_elicitation_complete("eid-1").await);
    }

    /// The frozen `2026-07-28` deleted `notifications/elicitation/complete`.
    /// A draft handle must stay silent rather than emit a notification the
    /// client's schema does not define.
    #[tokio::test]
    async fn elicitation_complete_is_a_no_op_on_the_draft_wire() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        let handle = ClientHandle::mrtr(
            Route::to(turbomcp_service::Peer::new("draft-elicit-conn", &tx)),
            None,
            BTreeMap::new(),
            None,
            false,
        );

        assert!(
            !handle.notify_elicitation_complete("eid-1").await,
            "the draft wire has no elicitation/complete"
        );
        assert!(
            rx.try_recv().is_err(),
            "nothing may reach the client on the draft wire"
        );
    }

    #[tokio::test]
    async fn strict_keys_reject_shape_conflict() {
        let handle = ClientHandle::mrtr(
            Route::default(),
            Some(json!({ "elicitation": {} })),
            BTreeMap::new(),
            None,
            true,
        );
        // First records under `k` and aborts (InputRequired).
        let _ = handle
            .elicit("k", neutral::ElicitParams::new("A", form_schema()))
            .await;
        // Same key, different request shape → strict error (not a warning).
        let err = handle
            .elicit(
                "k",
                neutral::ElicitParams::new(
                    "B",
                    json!({ "type": "object", "properties": { "other": { "type": "boolean" } } }),
                ),
            )
            .await
            .expect_err("strict keys reject a shape conflict");
        assert!(matches!(err, McpError::InvalidParams(_)));
    }

    // ---- requestState verification edges ------------------------------------

    /// A token sealed (with the sealer's own key) around `payload`. Lets a test
    /// reach the checks that run after decryption (expiry, shape), which
    /// `seal` can't be made to violate.
    fn crafted(sealer: &StateSealer, payload: &Value) -> String {
        crafted_bytes(sealer, &serde_json::to_vec(payload).expect("serializable"))
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
        let sealer = StateSealer::new();
        let good = sealer.seal("tools/call", None, &json!({ "a": 1 })).unwrap();
        let body = good.strip_prefix("v2.").unwrap().to_owned();

        for (what, token) in [
            ("empty", String::new()),
            ("no version prefix", body.clone()),
            ("unknown version", format!("v9.{body}")),
            ("the old signed format", format!("v1.{body}.{body}")),
            ("not base64", "v2.~~~~".to_owned()),
            ("shorter than a key id and nonce", "v2.AAAA".to_owned()),
            (
                "over the length bound",
                format!("v2.{}", "A".repeat(2 * DEFAULT_STATE_BYTES)),
            ),
            (
                "sealed, but not JSON",
                crafted_bytes(&sealer, b"not json at all"),
            ),
        ] {
            assert_uniform_rejection(sealer.open("tools/call", None, &token), what);
        }
    }

    /// As [`crafted`], for a payload that isn't valid JSON.
    fn crafted_bytes(sealer: &StateSealer, bytes: &[u8]) -> String {
        sealer.seal_bytes(bytes).expect("sealable")
    }

    /// The TTL is the replay bound (the mrtr spec's SHOULD). It is the one
    /// check `sign` cannot be coaxed into violating, so it needs a crafted
    /// token — and the far-future control rules out "the crafted token was
    /// simply malformed": identical construction, opposite verdict.
    #[test]
    fn an_expired_state_is_rejected_and_a_live_one_is_not() {
        let sealer = StateSealer::new();
        let payload =
            |exp: u64| json!({ "m": "tools/call", "sub": null, "exp": exp, "d": { "n": 7 } });

        assert_uniform_rejection(
            sealer.open("tools/call", None, &crafted(&sealer, &payload(1))),
            "expired in 1970",
        );
        assert_eq!(
            sealer
                .open("tools/call", None, &crafted(&sealer, &payload(u64::MAX)))
                .expect("an unexpired crafted token verifies"),
            json!({ "n": 7 }),
            "the control proves the rejection above was the expiry, not the shape"
        );
        // A payload with no `exp` at all is treated as expired, not as
        // "unbounded" — the absent field must not become a forever token.
        assert_uniform_rejection(
            sealer.open(
                "tools/call",
                None,
                &crafted(&sealer, &json!({ "m": "tools/call", "sub": null, "d": {} })),
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
        tokio::sync::mpsc::Sender<turbomcp_core::JsonRpcMessage>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let pending = Arc::new(PendingRequests::default());
        let handle = ClientHandle::bidi(
            Route::to(turbomcp_service::Peer::new(connection, &tx)),
            Arc::clone(&pending),
            // A fully-capable client, URL mode included — the bidi tests drive
            // both elicitation modes.
            Some(json!({
                "elicitation": { "form": {}, "url": {} },
                "sampling": { "context": {}, "tools": {} },
                "roots": {}
            })),
            ProtocolVersion::V2025_11_25,
        );
        (handle, pending, rx, tx)
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
                .elicit("k", neutral::ElicitParams::new("?", form_schema()))
                .await
        });

        let req = next_request(&mut rx).await;
        pending.complete(JsonRpcResponse::error(
            req.id,
            turbomcp_core::JsonRpcError {
                code: turbomcp_core::codes::METHOD_NOT_FOUND,
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
                .elicit("k", neutral::ElicitParams::new("?", form_schema()))
                .await
        });

        let req = next_request(&mut rx).await;
        pending.complete(JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: Some(req.id),
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
            Route::default(),
            pending,
            Some(json!({ "elicitation": {} })),
            ProtocolVersion::V2025_11_25,
        );
        let err = handle
            .elicit("k", neutral::ElicitParams::new("?", form_schema()))
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
                .elicit("k", neutral::ElicitParams::new("?", form_schema()))
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
                    ("first", neutral::ElicitParams::new("A", form_schema())),
                    ("second", neutral::ElicitParams::new("B", form_schema())),
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
            Route::default(),
            Some(json!({ "elicitation": {} })),
            BTreeMap::from([("first".to_owned(), json!({ "action": "accept" }))]),
            None,
            false,
        );
        let err = handle
            .elicit_all(vec![
                ("first", neutral::ElicitParams::new("A", form_schema())),
                ("second", neutral::ElicitParams::new("B", form_schema())),
                ("third", neutral::ElicitParams::new("C", form_schema())),
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
            Route::default(),
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
                ("a", neutral::ElicitParams::new("A", form_schema())),
                ("b", neutral::ElicitParams::new("B", form_schema())),
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

    /// Tool-enabled sampling needs `sampling.tools`, and context needs
    /// `sampling.context`.
    ///
    /// "Servers MUST NOT send tool-enabled sampling requests to Clients that
    /// have not declared support for tool use." The capability a request needs
    /// depends on what the request carries, so it is read off the params.
    #[tokio::test]
    #[allow(deprecated)] // still functional on both wires; see the method docs
    async fn tool_enabled_sampling_needs_the_declared_sub_capability() {
        let plain = ClientHandle::mrtr(
            Route::default(),
            Some(json!({ "sampling": {} })),
            BTreeMap::new(),
            None,
            false,
        );
        let offered = alloc_tool();
        for (label, params, want) in [
            (
                "tools",
                neutral::CreateMessageParams::new(Vec::new(), 16).with_tools(vec![offered.clone()]),
                "sampling.tools",
            ),
            (
                "toolChoice",
                neutral::CreateMessageParams::new(Vec::new(), 16)
                    .with_tool_choice(neutral::ToolChoice::Auto),
                "sampling.tools",
            ),
            (
                "includeContext",
                neutral::CreateMessageParams::new(Vec::new(), 16)
                    .with_include_context(neutral::IncludeContext::AllServers),
                "sampling.context",
            ),
        ] {
            let err = plain
                .create_message("k", params)
                .await
                .expect_err("undeclared sub-capability");
            assert!(
                matches!(&err, McpError::MissingRequiredCapability(c) if c == want),
                "{label} -> {err:?}"
            );
        }
        // Plain sampling, and the undeclared-safe context value, still go out.
        for params in [
            neutral::CreateMessageParams::new(Vec::new(), 16),
            neutral::CreateMessageParams::new(Vec::new(), 16)
                .with_include_context(neutral::IncludeContext::None),
        ] {
            assert!(matches!(
                plain.create_message("k", params).await,
                Err(McpError::InputRequired)
            ));
        }
        // And a client that declared tools gets them.
        let agentic = ClientHandle::mrtr(
            Route::default(),
            Some(json!({ "sampling": { "tools": {} } })),
            BTreeMap::new(),
            None,
            false,
        );
        assert!(matches!(
            agentic
                .create_message(
                    "k",
                    neutral::CreateMessageParams::new(Vec::new(), 16).with_tools(vec![offered])
                )
                .await,
            Err(McpError::InputRequired)
        ));
    }

    /// A request carrying both tools and `includeContext` needs both promises;
    /// only `sampling.tools` used to be checked.
    #[tokio::test]
    #[allow(deprecated)]
    async fn a_request_needing_two_sampling_capabilities_checks_both() {
        let tools_only = ClientHandle::mrtr(
            Route::default(),
            Some(json!({ "sampling": { "tools": {} } })),
            BTreeMap::new(),
            None,
            false,
        );
        let both = neutral::CreateMessageParams::new(Vec::new(), 16)
            .with_tools(vec![alloc_tool()])
            .with_include_context(neutral::IncludeContext::ThisServer);
        let err = tools_only.create_message("k", both).await.unwrap_err();
        assert!(
            matches!(&err, McpError::MissingRequiredCapability(c) if c == "sampling.context"),
            "{err:?}"
        );
    }

    /// "Servers SHOULD validate received data matches the requested schema."
    /// An accepted form is entirely client-supplied; on MRTR it is whatever
    /// the client put in `inputResponses`.
    #[tokio::test]
    async fn an_accepted_answer_that_misses_the_schema_is_refused() {
        let schema = json!({
            "type": "object",
            "properties": { "age": { "type": "integer", "minimum": 0 } },
            "required": ["age"]
        });
        let answered = |content: Value| {
            ClientHandle::mrtr(
                Route::default(),
                Some(json!({ "elicitation": {} })),
                BTreeMap::from([(
                    "age".to_owned(),
                    json!({ "action": "accept", "content": content }),
                )]),
                None,
                false,
            )
        };
        let bad = answered(json!({ "age": "DROP TABLE" }))
            .elicit("age", neutral::ElicitParams::new("Age?", schema.clone()))
            .await
            .unwrap_err();
        assert!(matches!(bad, McpError::InvalidParams(_)), "{bad:?}");

        let good = answered(json!({ "age": 41 }))
            .elicit("age", neutral::ElicitParams::new("Age?", schema.clone()))
            .await
            .unwrap();
        assert!(good.accepted());

        // A decline carries no content to check.
        let declined = ClientHandle::mrtr(
            Route::default(),
            Some(json!({ "elicitation": {} })),
            BTreeMap::from([("age".to_owned(), json!({ "action": "decline" }))]),
            None,
            false,
        );
        assert!(
            !declined
                .elicit("age", neutral::ElicitParams::new("Age?", schema))
                .await
                .unwrap()
                .accepted()
        );
    }

    /// `elicit_all` on MRTR used to skip the form-subset check `elicit` makes,
    /// so an unrenderable form went out batched in `inputRequests`.
    #[tokio::test]
    async fn elicit_all_refuses_an_unrenderable_form_like_elicit_does() {
        let handle = ClientHandle::mrtr(
            Route::default(),
            Some(json!({ "elicitation": {} })),
            BTreeMap::new(),
            None,
            false,
        );
        let nested = json!({
            "type": "object",
            "properties": { "tags": { "type": "array", "items": { "type": "object" } } }
        });
        let err = handle
            .elicit_all(vec![("a", neutral::ElicitParams::new("?", nested))])
            .await
            .unwrap_err();
        assert!(matches!(err, McpError::InvalidParams(_)), "{err:?}");
        assert!(handle.collected().is_empty(), "nothing went out");
    }

    /// A one-tool catalogue for the sampling tests.
    fn alloc_tool() -> neutral::Tool {
        neutral::Tool::new("echo", json!({ "type": "object" }))
    }

    /// A client that named its modes and left `form` out is not sent a form.
    ///
    /// "Clients declaring the `elicitation` capability MUST support at least
    /// one mode", so `{"elicitation": {"url": {}}}` is a legal declaration by a
    /// client that renders nothing — and a form strands the user there exactly
    /// as an undeclared capability would. Bare `elicitation` still means forms,
    /// on every revision: `2025-06-18` has no sub-capabilities to name, and a
    /// later client that named none has said nothing to the contrary.
    #[tokio::test]
    async fn a_url_only_client_is_not_sent_a_form() {
        let url_only = ClientHandle::mrtr(
            Route::default(),
            Some(json!({ "elicitation": { "url": {} } })),
            BTreeMap::new(),
            None,
            false,
        );
        let err = url_only
            .elicit("k", neutral::ElicitParams::new("?", form_schema()))
            .await
            .expect_err("this client renders no forms");
        assert!(
            matches!(&err, McpError::MissingRequiredCapability(c) if c == "elicitation.form"),
            "{err:?}"
        );

        // Declaring the mode, or naming none at all, both get forms.
        for caps in [
            json!({ "elicitation": { "form": {} } }),
            json!({ "elicitation": {} }),
        ] {
            let handle = ClientHandle::mrtr(
                Route::default(),
                Some(caps.clone()),
                BTreeMap::new(),
                None,
                false,
            );
            assert!(
                matches!(
                    handle
                        .elicit("k", neutral::ElicitParams::new("?", form_schema()))
                        .await,
                    Err(McpError::InputRequired)
                ),
                "{caps}"
            );
        }

        // And `2025-06-18` has no sub-capabilities to name, so a sub-capability
        // can never be required of it.
        let older = ClientHandle::bidi(
            Route::default(),
            Arc::new(PendingRequests::default()),
            Some(json!({ "elicitation": {} })),
            ProtocolVersion::V2025_06_18,
        );
        let err = older
            .elicit("k", neutral::ElicitParams::new("?", form_schema()))
            .await
            .expect_err("no writer is registered");
        assert!(matches!(err, McpError::Transport(_)), "{err:?}");
    }

    /// A form a client cannot render never leaves the server, and neither does
    /// a URL it cannot navigate to.
    ///
    /// Both are things the user would otherwise meet as silence: an empty form
    /// with no explanation, or a link that resolves against whatever the
    /// client's own UI happens to be.
    #[tokio::test]
    async fn an_unrenderable_elicitation_is_refused_before_it_is_sent() {
        let handle = ClientHandle::mrtr(
            Route::default(),
            Some(json!({ "elicitation": { "form": {}, "url": {} } })),
            BTreeMap::new(),
            None,
            false,
        );

        // "Form mode elicitation schemas are limited to flat objects with
        // primitive properties only."
        let nested = json!({
            "type": "object",
            "properties": { "address": { "type": "object", "properties": {} } },
        });
        let err = handle
            .elicit("k", neutral::ElicitParams::new("?", nested))
            .await
            .expect_err("a nested schema is outside the subset");
        assert!(
            matches!(&err, McpError::InvalidParams(m) if m.contains("address")),
            "{err:?}"
        );

        // "The `url` parameter MUST contain a valid URL."
        for bad in [
            "not a url",
            "/relative/path",
            "javascript:alert(1)",
            "https:no-host",
            "https:///path",
            "ftp://files.example/x",
        ] {
            let err = handle
                .elicit_url("k", neutral::ElicitUrlParams::new("Sign in", bad))
                .await
                .expect_err("not somewhere to send a user");
            assert!(matches!(&err, McpError::InvalidParams(_)), "{bad}: {err:?}");
        }

        // A real one gets as far as being recorded for the retry (schemes are
        // case-insensitive).
        assert!(matches!(
            handle
                .elicit_url(
                    "k",
                    neutral::ElicitUrlParams::new("Sign in", "HTTPS://auth.example/go")
                )
                .await,
            Err(McpError::InputRequired)
        ));
        assert!(matches!(
            handle
                .elicit_url(
                    "k",
                    neutral::ElicitUrlParams::new("Sign in", "https://auth.example/go")
                )
                .await,
            Err(McpError::InputRequired)
        ));
    }

    /// `2025-06-18` has no agentic sampling and no multi-block messages.
    ///
    /// Before the handle knew its revision, `Bidi` meant "legacy" and both
    /// wires got the `2025-11-25` shape: a `2025-06-18` client was handed
    /// `tools`, `toolChoice` and content arrays its schema does not define.
    #[tokio::test]
    #[allow(deprecated)] // still functional on every wire; see the method docs
    async fn the_2025_06_18_wire_refuses_what_it_cannot_express() {
        let caps = json!({ "sampling": { "tools": {}, "context": {} } });
        let older = ClientHandle::bidi(
            Route::default(),
            Arc::new(PendingRequests::default()),
            Some(caps.clone()),
            ProtocolVersion::V2025_06_18,
        );
        let multi = neutral::SamplingMessage::new(
            neutral::Role::User,
            vec![
                neutral::SamplingContent::text("look at this"),
                neutral::SamplingContent::image("aGk=", "image/png"),
            ],
        );
        for params in [
            neutral::CreateMessageParams::new(Vec::new(), 16).with_tools(vec![alloc_tool()]),
            neutral::CreateMessageParams::new(vec![multi.clone()], 16),
        ] {
            let err = older
                .create_message("k", params)
                .await
                .expect_err("2025-06-18 cannot carry this");
            assert!(
                matches!(&err, McpError::InvalidParams(m) if m.contains("2025-06-18")),
                "{err:?}"
            );
        }

        // The same requests are fine on 2025-11-25: they get as far as the
        // missing server→client channel, which is the next failure along.
        let newer = ClientHandle::bidi(
            Route::default(),
            Arc::new(PendingRequests::default()),
            Some(caps),
            ProtocolVersion::V2025_11_25,
        );
        for params in [
            neutral::CreateMessageParams::new(Vec::new(), 16).with_tools(vec![alloc_tool()]),
            neutral::CreateMessageParams::new(vec![multi], 16),
        ] {
            let err = newer
                .create_message("k", params)
                .await
                .expect_err("no writer is registered");
            assert!(matches!(err, McpError::Transport(_)), "{err:?}");
        }
    }

    /// The spec's two tool-use MUSTs are enforced before anything is sent.
    #[tokio::test]
    #[allow(deprecated)] // still functional on every wire; see the method docs
    async fn an_unbalanced_tool_conversation_never_reaches_the_client() {
        let handle = ClientHandle::mrtr(
            Route::default(),
            Some(json!({ "sampling": { "tools": {} } })),
            BTreeMap::new(),
            None,
            false,
        );
        let call = neutral::SamplingMessage::new(
            neutral::Role::Assistant,
            vec![neutral::SamplingContent::tool_use(neutral::ToolUse::new(
                "call-1", "echo",
            ))],
        );

        // A tool call the conversation never answers.
        let err = handle
            .create_message(
                "k",
                neutral::CreateMessageParams::new(vec![call.clone()], 16),
            )
            .await
            .expect_err("unanswered tool call");
        assert!(
            matches!(&err, McpError::InvalidParams(m) if m.contains("never answers")),
            "{err:?}"
        );

        // An answer that mixes a tool result with other content.
        let mixed = neutral::SamplingMessage::new(
            neutral::Role::User,
            vec![
                neutral::SamplingContent::tool_result(neutral::ToolResult::new(
                    "call-1",
                    vec![neutral::Content::text("42")],
                )),
                neutral::SamplingContent::text("and also"),
            ],
        );
        let err = handle
            .create_message(
                "k",
                neutral::CreateMessageParams::new(vec![call.clone(), mixed], 16),
            )
            .await
            .expect_err("mixed tool-result message");
        assert!(
            matches!(&err, McpError::InvalidParams(m) if m.contains("must carry nothing else")),
            "{err:?}"
        );

        // Balanced: the call is answered by a results-only message, so the
        // request gets as far as being recorded for the retry.
        let answer = neutral::SamplingMessage::new(
            neutral::Role::User,
            vec![neutral::SamplingContent::tool_result(
                neutral::ToolResult::new("call-1", vec![neutral::Content::text("42")]),
            )],
        );
        assert!(matches!(
            handle
                .create_message(
                    "k",
                    neutral::CreateMessageParams::new(vec![call, answer], 16)
                )
                .await,
            Err(McpError::InputRequired)
        ));
    }

    /// A form-only client is not sent somewhere it cannot go.
    ///
    /// `elicitation` and `elicitation.url` are different declarations: the
    /// sub-capability says the client can hand the user off to a consent page.
    /// Checking only the top-level key sent URL mode to clients that render
    /// forms and nothing else, which strands the interaction exactly as
    /// sending an undeclared capability would.
    #[tokio::test]
    async fn url_mode_is_refused_when_only_form_was_declared() {
        let handle = ClientHandle::mrtr(
            Route::default(),
            Some(json!({ "elicitation": { "form": {} } })),
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
            .expect_err("a form-only client cannot open a URL");
        assert!(
            matches!(&err, McpError::MissingRequiredCapability(c) if c == "elicitation.url"),
            "{err:?}"
        );
        // Form mode still works: `2025-06-18` has no sub-capabilities at all,
        // so bare `elicitation` has to keep meaning "I can render a form".
        let form_only = ClientHandle::mrtr(
            Route::default(),
            Some(json!({ "elicitation": {} })),
            BTreeMap::new(),
            None,
            false,
        );
        assert!(matches!(
            form_only
                .elicit("k", neutral::ElicitParams::new("?", form_schema()))
                .await,
            Err(McpError::InputRequired)
        ));
    }

    /// URL-mode elicitation resolves from the retry's cached response too —
    /// the path a real OAuth consent round trip returns on.
    #[tokio::test]
    async fn elicit_url_resolves_from_the_retry_response() {
        let handle = ClientHandle::mrtr(
            Route::default(),
            Some(json!({ "elicitation": { "url": {} } })),
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
            Route::default(),
            Some(json!({ "sampling": {}, "roots": {} })),
            BTreeMap::new(),
            None,
            false,
        );
        assert!(matches!(
            handle
                .create_message(
                    "s",
                    neutral::CreateMessageParams::new(
                        vec![neutral::SamplingMessage::text(neutral::Role::User, "hi")],
                        64,
                    ),
                )
                .await,
            Err(McpError::InputRequired)
        ));
        assert!(matches!(
            handle.list_roots("r").await,
            Err(McpError::InputRequired)
        ));

        let collected = handle.collected();
        assert_eq!(collected["s"]["method"], "sampling/createMessage");
        assert_eq!(
            collected["s"]["params"],
            json!({
                "messages": [{ "role": "user", "content": { "type": "text", "text": "hi" } }],
                "maxTokens": 64,
            }),
            "a lone content block renders bare, which every revision reads"
        );
        assert_eq!(collected["r"]["method"], "roots/list");

        // Undeclared is refused (SEP-2322 MUST NOT), per capability.
        let bare = ClientHandle::mrtr(
            Route::default(),
            Some(json!({ "roots": {} })),
            BTreeMap::new(),
            None,
            false,
        );
        assert!(matches!(
            bare.create_message("s", neutral::CreateMessageParams::new(Vec::new(), 16))
                .await,
            Err(McpError::MissingRequiredCapability(c)) if c == "sampling"
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
            .elicit("k", neutral::ElicitParams::new("?", form_schema()))
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
            .elicit("k", neutral::ElicitParams::new("?", form_schema()))
            .await
            .expect("the broker answered");
        assert_eq!(outcome.content["via"], "k");

        // `elicit_all` resolves through the broker one at a time as well.
        let outcomes = handle
            .elicit_all(vec![
                ("a", neutral::ElicitParams::new("A", form_schema())),
                ("b", neutral::ElicitParams::new("B", form_schema())),
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
                .elicit("k", neutral::ElicitParams::new("?", form_schema()))
                .await
                .expect_err("unavailable"),
            handle
                .elicit_all(vec![("k", neutral::ElicitParams::new("?", form_schema()))])
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

        let handle = ClientHandle::mrtr(Route::default(), None, BTreeMap::new(), None, false);
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
        let retry = ClientHandle::mrtr(Route::default(), None, BTreeMap::new(), Some(out), false);
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
        let null_state = ClientHandle::mrtr(
            Route::default(),
            None,
            BTreeMap::new(),
            Some(Value::Null),
            false,
        );
        assert!(null_state.load_state::<Resume>().unwrap().is_none());
    }

    /// "Store once, load on retry": a round that reads the state without
    /// storing it again must still pass it on. It used to start each round
    /// empty, so the third execution of a two-question handler saw no state
    /// and redid whatever the first had done.
    #[test]
    fn resume_state_survives_a_round_that_does_not_store_it() {
        let first = ClientHandle::mrtr(Route::default(), None, BTreeMap::new(), None, false);
        first.store_state(&"created-record-7").unwrap();
        let second = ClientHandle::mrtr(
            Route::default(),
            None,
            BTreeMap::new(),
            first.state_out(),
            false,
        );
        assert_eq!(
            second.load_state::<String>().unwrap().as_deref(),
            Some("created-record-7")
        );
        // Round two only reads it.
        let third = ClientHandle::mrtr(
            Route::default(),
            None,
            BTreeMap::new(),
            second.state_out(),
            false,
        );
        assert_eq!(
            third.load_state::<String>().unwrap().as_deref(),
            Some("created-record-7")
        );

        third.clear_state();
        let fourth = ClientHandle::mrtr(
            Route::default(),
            None,
            BTreeMap::new(),
            third.state_out(),
            false,
        );
        assert!(
            fourth.load_state::<String>().unwrap().is_none(),
            "cleared means gone"
        );
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
            Route::default(),
            Some(json!({ "elicitation": {} })),
            BTreeMap::new(),
            None,
            false,
        );
        let _ = handle
            .elicit("k", neutral::ElicitParams::new("A", form_schema()))
            .await;
        // A conflicting reshape aborts with InputRequired (warn), not InvalidParams.
        let err = handle
            .elicit(
                "k",
                neutral::ElicitParams::new(
                    "B",
                    json!({ "type": "object", "properties": { "other": { "type": "boolean" } } }),
                ),
            )
            .await
            .expect_err("still aborts");
        assert!(matches!(err, McpError::InputRequired));
    }
}
