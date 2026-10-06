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
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;
use turbomcp_client::{
    ClientError, ClientResult, ElicitationHandler, RootsHandler, SamplingHandler, async_trait,
};
use turbomcp_core::{JsonRpcError, McpError, ProtocolVersion};
use turbomcp_protocol::neutral;
use turbomcp_server::ClientHandle;

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
}

impl Bridge {
    pub(crate) fn new(
        handle: ClientHandle,
        version: ProtocolVersion,
        completions: Completions,
    ) -> Self {
        Self {
            inner: Arc::new(BridgeInner {
                handle,
                aborted: Notify::new(),
                asked: Mutex::new(HashMap::new()),
                version,
                completions,
            }),
        }
    }

    /// Resolves once asking aborted the downstream call.
    pub(crate) async fn aborted(&self) {
        self.inner.aborted.notified().await;
    }

    /// The question's key: its kind, content and how many times this
    /// execution asked it, identical on the downstream's retry.
    fn key(&self, kind: &str, question: &impl core::fmt::Debug) -> String {
        let mut hasher = DefaultHasher::new();
        format!("{question:?}").hash(&mut hasher);
        let digest = hasher.finish();
        let mut asked = self.inner.asked.lock().expect("bridge lock");
        let n = asked.entry(digest).or_insert(0);
        *n += 1;
        format!("upstream.{kind}.{digest:016x}.{n}")
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

    fn client_error(&self, error: &McpError) -> ClientError {
        ClientError::Rpc(error.to_jsonrpc_error(&self.inner.version))
    }
}

#[async_trait]
impl ElicitationHandler for Bridge {
    async fn elicit(&self, request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        let key = self.key("elicit", &request);
        let asked = self.inner.handle.elicit(&key, request).await;
        match self.settle(asked).await {
            Ok(outcome) => outcome,
            Err(e) => {
                tracing::warn!(error = %e, "could not ask the caller; the upstream gets a cancel");
                neutral::ElicitOutcome::new(neutral::ElicitAction::Cancel, Default::default())
            }
        }
    }

    fn supports_url_mode(&self) -> bool {
        true
    }

    async fn elicit_url(&self, request: neutral::ElicitUrlParams) -> neutral::ElicitOutcome {
        // Keyed by order alone: an upstream may mint a fresh elicitation id
        // (or URL token) every time it runs, and the answer carries no
        // content a mismatch could misplace.
        let key = self.key("elicit_url", &());
        if let Some(id) = &request.elicitation_id {
            self.inner
                .completions
                .insert(id.clone(), self.inner.handle.clone());
        }
        let asked = self.inner.handle.elicit_url(&key, request).await;
        match self.settle(asked).await {
            Ok(outcome) => outcome,
            Err(e) => {
                tracing::warn!(error = %e, "could not ask the caller; the upstream gets a cancel");
                neutral::ElicitOutcome::new(neutral::ElicitAction::Cancel, Default::default())
            }
        }
    }
}

#[async_trait]
impl SamplingHandler for Bridge {
    async fn create_message(
        &self,
        params: neutral::CreateMessageParams,
    ) -> ClientResult<neutral::CreateMessageResult> {
        let key = self.key("sample", &params);
        #[allow(deprecated)] // Deprecated upstream, still in every revision.
        let asked = self.inner.handle.create_message(&key, params).await;
        self.settle(asked).await.map_err(|e| self.client_error(&e))
    }

    fn capability(&self) -> neutral::SamplingCapability {
        full_sampling()
    }
}

#[async_trait]
impl RootsHandler for Bridge {
    async fn list_roots(&self) -> ClientResult<Vec<neutral::Root>> {
        let key = self.key("roots", &"roots/list");
        #[allow(deprecated)] // Deprecated upstream, still in every revision.
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
        tracing::warn!("refusing an upstream elicitation no caller claims");
        neutral::ElicitOutcome::new(neutral::ElicitAction::Cancel, Default::default())
    }

    fn supports_url_mode(&self) -> bool {
        true
    }

    async fn elicit_url(&self, _request: neutral::ElicitUrlParams) -> neutral::ElicitOutcome {
        tracing::warn!("refusing an upstream URL elicitation no caller claims");
        neutral::ElicitOutcome::new(neutral::ElicitAction::Cancel, Default::default())
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
    async fn list_roots(&self) -> ClientResult<Vec<neutral::Root>> {
        Err(ClientError::Rpc(unattributed()))
    }
}
