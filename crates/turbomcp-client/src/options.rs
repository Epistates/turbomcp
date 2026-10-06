//! [`CallOptions`]: what one call can ask for beyond its arguments.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Map, Value};
use turbomcp_core::{CancellationToken, LogLevel, meta::keys};
use turbomcp_protocol::neutral;

use crate::error::{ClientError, ClientResult};
use crate::handler::{ClientHandlers, ElicitationHandler, RootsHandler, SamplingHandler};
use crate::progress::ProgressCallback;

/// Per-call options for [`Client::call_tool_with`](crate::Client::call_tool_with),
/// [`read_resource_with`](crate::Client::read_resource_with) and
/// [`get_prompt_with`](crate::Client::get_prompt_with).
///
/// ```no_run
/// # use std::time::Duration;
/// # use turbomcp_client::{CallOptions, Client};
/// # async fn run(client: &Client) -> turbomcp_client::ClientResult<()> {
/// let options = CallOptions::new()
///     .timeout(Duration::from_secs(10))
///     .reset_timeout_on_progress(true)
///     .max_total_timeout(Duration::from_secs(300))
///     .on_progress(|p| eprintln!("{:.0}%", p.fraction().unwrap_or(0.0) * 100.0));
/// let result = client
///     .call_tool_with("upload", serde_json::Map::new(), &options)
///     .await?;
/// # Ok(()) }
/// ```
#[derive(Clone, Default)]
#[non_exhaustive]
pub struct CallOptions {
    pub(crate) timeout: Option<Duration>,
    pub(crate) max_total_timeout: Option<Duration>,
    pub(crate) reset_timeout_on_progress: bool,
    pub(crate) on_progress: Option<ProgressCallback>,
    pub(crate) meta: Map<String, Value>,
    pub(crate) log_level: Option<LogLevel>,
    pub(crate) cancel: Option<CancellationToken>,
    pub(crate) task: Option<Option<i64>>,
    pub(crate) input: Option<ClientHandlers>,
}

impl core::fmt::Debug for CallOptions {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CallOptions")
            .field("timeout", &self.timeout)
            .field("max_total_timeout", &self.max_total_timeout)
            .field("reset_timeout_on_progress", &self.reset_timeout_on_progress)
            .field("on_progress", &self.on_progress.is_some())
            .field("meta", &self.meta)
            .field("log_level", &self.log_level)
            .field("cancel", &self.cancel.is_some())
            .field("task", &self.task)
            .field("input", &self.input)
            .finish()
    }
}

/// `_meta` keys the client writes itself.
const RESERVED_META: &[&str] = &[
    keys::PROTOCOL_VERSION,
    keys::CLIENT_INFO,
    keys::CLIENT_CAPABILITIES,
    keys::PROGRESS_TOKEN,
    keys::LOG_LEVEL,
    keys::SUBSCRIPTION_ID,
];

impl CallOptions {
    /// No options: the client's defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Wait this long for each request of the call, instead of the client's
    /// timeout. (A `2026-07-28` call that asks the user something is several
    /// requests.)
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Restart the timeout whenever the server reports progress, so a long
    /// operation that keeps reporting isn't cut off. Pair with
    /// [`max_total_timeout`](Self::max_total_timeout): the spec asks for a
    /// ceiling either way.
    #[must_use]
    pub fn reset_timeout_on_progress(mut self, reset: bool) -> Self {
        self.reset_timeout_on_progress = reset;
        self
    }

    /// Give up on the whole call after this long, whatever progress arrives.
    #[must_use]
    pub fn max_total_timeout(mut self, max: Duration) -> Self {
        self.max_total_timeout = Some(max);
        self
    }

    /// Ask the server to report progress, and hand each update to `callback`.
    ///
    /// The client mints the progress token, unique among its in-flight
    /// requests as the spec requires, and routes this call's updates here and
    /// only here. `callback` runs on the connection's reader, in order: keep it
    /// cheap (send to a channel) rather than block.
    #[must_use]
    pub fn on_progress(
        mut self,
        callback: impl Fn(neutral::Progress) + Send + Sync + 'static,
    ) -> Self {
        self.on_progress = Some(Arc::new(callback));
        self
    }

    /// Add `key` to the request's `_meta`: trace context
    /// (`traceparent`, `tracestate`, `baggage`) or a key of your own. The keys
    /// the client writes itself (protocol version, client info and
    /// capabilities, progress token, log level) are refused when the call is
    /// made; use the matching option instead.
    #[must_use]
    pub fn meta(mut self, key: impl Into<String>, value: Value) -> Self {
        self.meta.insert(key.into(), value);
        self
    }

    /// Ask for server log messages at `level` and above for this call
    /// (`2026-07-28`, where the level is per request). Overrides
    /// `ClientBuilder::with_log_level`. A stateful session sets its level for
    /// the whole session with `Client::set_level`, so this is refused there.
    #[must_use]
    pub fn log_level(mut self, level: LogLevel) -> Self {
        self.log_level = Some(level);
        self
    }

    /// Abandon the call when `token` fires: the call returns
    /// [`ClientError::Cancelled`] and the server is told to stop.
    #[must_use]
    pub fn cancel_on(mut self, token: CancellationToken) -> Self {
        self.cancel = Some(token);
        self
    }

    /// Ask for task-augmented execution, with an optional retention window
    /// (`tools/call` only). See
    /// [`Client::call_tool_task`](crate::Client::call_tool_task).
    #[must_use]
    pub fn task(mut self, ttl_ms: Option<i64>) -> Self {
        self.task = Some(ttl_ms);
        self
    }

    /// Answer this call's `elicitation/create` requests with `handler`,
    /// instead of the client's own: the ones that belong to this call (see
    /// SEP-2260; on `2026-07-28` they come back in the call's own result, on
    /// Streamable HTTP on its own stream). The server sends only what the
    /// client declared at the handshake, so this answers for a capability
    /// the client already declares (register a client-wide handler for
    /// that); it doesn't add one.
    #[must_use]
    pub fn with_elicitation<H: ElicitationHandler>(mut self, handler: H) -> Self {
        self.input
            .get_or_insert_with(ClientHandlers::default)
            .elicitation = Some(Arc::new(handler));
        self
    }

    /// Answer this call's `sampling/createMessage` requests with `handler`.
    /// See [`with_elicitation`](Self::with_elicitation).
    #[must_use]
    pub fn with_sampling<H: SamplingHandler>(mut self, handler: H) -> Self {
        self.input
            .get_or_insert_with(ClientHandlers::default)
            .sampling = Some(Arc::new(handler));
        self
    }

    /// Answer this call's `roots/list` requests with `handler`. See
    /// [`with_elicitation`](Self::with_elicitation).
    #[must_use]
    pub fn with_roots<H: RootsHandler>(mut self, handler: H) -> Self {
        self.input.get_or_insert_with(ClientHandlers::default).roots = Some(Arc::new(handler));
        self
    }

    /// The caller's `_meta`, refusing a key the client owns.
    pub(crate) fn checked_meta(&self) -> ClientResult<Map<String, Value>> {
        if let Some(key) = self
            .meta
            .keys()
            .find(|k| RESERVED_META.contains(&k.as_str()))
        {
            return Err(ClientError::Protocol(format!(
                "`{key}` in `_meta` is written by the client; use the matching CallOptions setting"
            )));
        }
        Ok(self.meta.clone())
    }

    /// Whether the call needs a progress token.
    pub(crate) fn wants_progress(&self) -> bool {
        self.on_progress.is_some() || self.reset_timeout_on_progress
    }
}
