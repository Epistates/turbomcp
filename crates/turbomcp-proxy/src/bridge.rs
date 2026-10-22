//! Input bridging: an upstream's `elicitation/create`,
//! `sampling/createMessage` and `roots/list` asked of the downstream caller
//! whose call caused them, across delivery models.
//!
//! The upstream client attributes each request to the call it belongs to
//! (SEP-2260: on `2026-07-28` in the call's own result, on Streamable HTTP by
//! its stream, by `related-task` for a task) and hands it to that call's
//! [`Bridge`], which asks the downstream through the call's own
//! [`ClientHandle`]. Every pairing of revisions works:
//!
//! - **Downstream `2025-*`** (inline requests on its session): the bridge
//!   asks and waits, and the upstream gets the answer.
//! - **Downstream `2026-07-28`** (MRTR): asking aborts the downstream call
//!   with the question in its `InputRequiredResult`. The proxied call ends
//!   (which cancels the upstream request) and the downstream retries with
//!   the answer; the call runs again, the upstream asks the same question,
//!   and this time the bridge answers it at once. Questions are keyed by
//!   their content and order, so the retry finds its answer.
//!
//! A request the upstream client can't attribute (several calls in flight
//! on a transport that doesn't say whose it is) is refused, never shown to
//! a caller it may not belong to.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use tokio::sync::Notify;
use turbomcp_client::{
    ClientError, ClientResult, ElicitationHandler, RootsHandler, SamplingHandler, async_trait,
};
use turbomcp_core::{JsonRpcError, McpError, ProtocolVersion};
use turbomcp_protocol::neutral;
use turbomcp_server::ClientHandle;

/// The most distinct questions one proxied call may ask (see
/// [`Bridge::key`]).
const MAX_QUESTIONS: usize = 16;

/// FNV-1a over `bytes`: a stable, non-cryptographic digest for question keys.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Asks one downstream call's caller what its upstream asks.
#[derive(Clone)]
pub(crate) struct Bridge {
    inner: Arc<BridgeInner>,
}

struct BridgeInner {
    handle: ClientHandle,
    /// Fired when asking aborted the downstream call (MRTR).
    aborted: Notify,
    /// How many times each question has been asked in this execution.
    asked: Mutex<HashMap<u64, u32>>,
    /// The downstream revision, to render errors for it.
    version: ProtocolVersion,
    /// Where an upstream's URL elicitations finish.
    completions: Completions,
    /// The upstream's name, put before the elicitations it sends.
    attribution: Option<Arc<str>>,
}

impl Bridge {
    pub(crate) fn new(
        handle: ClientHandle,
        version: ProtocolVersion,
        completions: Completions,
        attribution: Option<Arc<str>>,
    ) -> Self {
        Self {
            inner: Arc::new(BridgeInner {
                handle,
                aborted: Notify::new(),
                asked: Mutex::new(HashMap::new()),
                version,
                completions,
                attribution,
            }),
        }
    }

    /// Resolves once asking aborted the downstream call.
    pub(crate) async fn aborted(&self) {
        self.inner.aborted.notified().await;
    }

    /// The question's key: its kind, content and how many times this
    /// execution asked it, identical on the downstream's retry.
    ///
    /// The digest is FNV-1a, a fixed algorithm, so a retry that lands on a
    /// replica built with another Rust (whose `DefaultHasher` is unspecified
    /// across versions) still finds its answer.
    ///
    /// # Errors
    /// Past [`MAX_QUESTIONS`] distinct questions in one execution. A retry
    /// re-runs the upstream call, so an upstream that asks something new
    /// every run asks one more each round: this bounds the rounds, where a
    /// hostile or volatile upstream (a timestamp in its message) would
    /// otherwise keep the caller retrying forever.
    fn key(&self, kind: &str, question: &impl core::fmt::Debug) -> ClientResult<String> {
        let digest = fnv1a(format!("{kind}:{question:?}").as_bytes());
        let mut asked = self.inner.asked.lock();
        if !asked.contains_key(&digest) && asked.len() >= MAX_QUESTIONS {
            return Err(ClientError::Rpc(JsonRpcError {
                code: turbomcp_core::codes::INTERNAL_ERROR,
                message: format!(
                    "the upstream asked more than {MAX_QUESTIONS} questions in one call; giving up"
                ),
                data: None,
            }));
        }
        let n = asked.entry(digest).or_insert(0);
        *n += 1;
        Ok(format!("upstream.{kind}.{digest:016x}.{n}"))
    }

    /// What asking the downstream came to, for the upstream: the answer, or
    /// (an MRTR abort) nothing ever, once the proxied call has been told to
    /// end.
    async fn settle<T>(&self, asked: Result<T, McpError>) -> Result<T, McpError> {
        match asked {
            Err(McpError::InputRequired) => {
                self.inner.aborted.notify_one();
                std::future::pending().await
            }
            other => other,
        }
    }

    /// `message` as the caller sees it: named after its upstream when the
    /// gateway attributes questions.
    fn attributed(&self, message: String) -> String {
        match &self.inner.attribution {
            Some(upstream) => format!("{upstream}: {message}"),
            None => message,
        }
    }

    fn client_error(&self, error: &McpError) -> ClientError {
        ClientError::Rpc(error.to_jsonrpc_error(&self.inner.version))
    }
}

/// The caller's answer, or why there is none, as the upstream should hear
/// it: a caller that can't elicit (or a question that failed to reach it) is
/// a JSON-RPC error, which the upstream can tell from a user who cancelled.
#[async_trait]
impl ElicitationHandler for Bridge {
    async fn elicit(&self, request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        self.ask_form(request).await.unwrap_or_else(|_| cancelled())
    }

    async fn elicit_with(
        &self,
        _ctx: &turbomcp_client::HandlerContext,
        request: neutral::ElicitParams,
    ) -> ClientResult<neutral::ElicitOutcome> {
        self.ask_form(request).await
    }

    fn supports_url_mode(&self) -> bool {
        true
    }

    async fn elicit_url(&self, request: neutral::ElicitUrlParams) -> neutral::ElicitOutcome {
        self.ask_url(request).await.unwrap_or_else(|_| cancelled())
    }

    async fn elicit_url_with(
        &self,
        _ctx: &turbomcp_client::HandlerContext,
        request: neutral::ElicitUrlParams,
    ) -> ClientResult<neutral::ElicitOutcome> {
        self.ask_url(request).await
    }
}

impl Bridge {
    async fn ask_form(
        &self,
        mut request: neutral::ElicitParams,
    ) -> ClientResult<neutral::ElicitOutcome> {
        let key = self.key("elicit", &request)?;
        request.message = self.attributed(request.message);
        let asked = self.inner.handle.elicit(&key, request).await;
        self.settle(asked).await.map_err(|e| self.client_error(&e))
    }

    async fn ask_url(
        &self,
        mut request: neutral::ElicitUrlParams,
    ) -> ClientResult<neutral::ElicitOutcome> {
        // Keyed by order alone: an upstream may mint a fresh elicitation id
        // (or URL token) every time it runs, and the answer carries no
        // content a mismatch could misplace.
        let key = self.key("elicit_url", &())?;
        if let Some(id) = &request.elicitation_id {
            self.inner
                .completions
                .insert(id.clone(), self.inner.handle.clone());
        }
        request.message = self.attributed(request.message);
        let asked = self.inner.handle.elicit_url(&key, request).await;
        self.settle(asked).await.map_err(|e| self.client_error(&e))
    }
}

/// A cancel, for the plain handler methods nothing here calls (the client
/// calls the `*_with` ones, which report errors as errors).
fn cancelled() -> neutral::ElicitOutcome {
    neutral::ElicitOutcome::new(neutral::ElicitAction::Cancel, Default::default())
}

#[async_trait]
impl SamplingHandler for Bridge {
    async fn create_message(
        &self,
        params: neutral::CreateMessageParams,
    ) -> ClientResult<neutral::CreateMessageResult> {
        let key = self.key("sample", &params)?;
        let asked = self.inner.handle.create_message(&key, params).await;
        self.settle(asked).await.map_err(|e| self.client_error(&e))
    }

    fn capability(&self) -> neutral::SamplingCapability {
        full_sampling()
    }
}

#[async_trait]
impl RootsHandler for Bridge {
    async fn list_roots(&self) -> ClientResult<neutral::ListRootsResult> {
        let key = self.key("roots", &"roots/list")?;
        let asked = self.inner.handle.list_roots(&key).await;
        self.settle(asked).await.map_err(|e| self.client_error(&e))
    }
}

/// Every sampling feature: what the downstream caller actually offers is
/// checked when the bridge asks it.
fn full_sampling() -> neutral::SamplingCapability {
    neutral::SamplingCapability::new()
        .with_context(true)
        .with_tools(true)
}

/// The URL elicitations in progress, by the upstream's elicitation id: where
/// its `notifications/elicitation/complete` goes. Outlives the call (the
/// flow finishes out of band), so bounded and expiring.
pub(crate) type Completions = moka::sync::Cache<String, ClientHandle>;

/// The completions registry for one upstream connection.
pub(crate) fn completions() -> Completions {
    moka::sync::Cache::builder()
        .max_capacity(10_000)
        .time_to_live(std::time::Duration::from_secs(60 * 60))
        .build()
}

/// The upstream client's own handlers: what declares the capabilities the
/// bridge answers for, what answers a request no call claims (a refusal,
/// never a guess), and where URL elicitations finish.
#[derive(Clone)]
pub(crate) struct UpstreamHandlers {
    pub(crate) completions: Completions,
}

fn unattributed() -> JsonRpcError {
    JsonRpcError {
        code: turbomcp_core::codes::INTERNAL_ERROR,
        message: "the proxy cannot tell which caller this request belongs to".into(),
        data: None,
    }
}

#[async_trait]
impl ElicitationHandler for UpstreamHandlers {
    async fn elicit(&self, _request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        neutral::ElicitOutcome::new(neutral::ElicitAction::Cancel, Default::default())
    }

    /// No caller claims it: refused as such, not answered as a user's cancel.
    async fn elicit_with(
        &self,
        _ctx: &turbomcp_client::HandlerContext,
        _request: neutral::ElicitParams,
    ) -> ClientResult<neutral::ElicitOutcome> {
        tracing::warn!("refusing an upstream elicitation no caller claims");
        Err(ClientError::Rpc(unattributed()))
    }

    fn supports_url_mode(&self) -> bool {
        true
    }

    async fn elicit_url(&self, _request: neutral::ElicitUrlParams) -> neutral::ElicitOutcome {
        neutral::ElicitOutcome::new(neutral::ElicitAction::Cancel, Default::default())
    }

    async fn elicit_url_with(
        &self,
        _ctx: &turbomcp_client::HandlerContext,
        _request: neutral::ElicitUrlParams,
    ) -> ClientResult<neutral::ElicitOutcome> {
        tracing::warn!("refusing an upstream URL elicitation no caller claims");
        Err(ClientError::Rpc(unattributed()))
    }

    async fn on_elicitation_complete(&self, elicitation_id: String) {
        if let Some(handle) = self.completions.remove(&elicitation_id) {
            handle.notify_elicitation_complete(&elicitation_id).await;
        }
    }
}

#[async_trait]
impl SamplingHandler for UpstreamHandlers {
    async fn create_message(
        &self,
        _params: neutral::CreateMessageParams,
    ) -> ClientResult<neutral::CreateMessageResult> {
        Err(ClientError::Rpc(unattributed()))
    }

    fn capability(&self) -> neutral::SamplingCapability {
        full_sampling()
    }
}

#[async_trait]
impl RootsHandler for UpstreamHandlers {
    async fn list_roots(&self) -> ClientResult<neutral::ListRootsResult> {
        Err(ClientError::Rpc(unattributed()))
    }
}

#[cfg(test)]
mod tests {
    use super::fnv1a;

    /// The published FNV-1a 64 vectors: the key a retry looks up must not
    /// depend on which build computes it.
    #[test]
    fn question_keys_use_the_published_fnv1a() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
    }
}
