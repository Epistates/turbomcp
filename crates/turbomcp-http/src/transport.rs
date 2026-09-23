//! MCP 2025-11-25 Compliant Streamable HTTP Client - Standard Implementation
//!
//! This client provides **strict MCP 2025-11-25 specification compliance** with:
//! - Single MCP endpoint for all communication
//! - Accept header negotiation (application/json, text/event-stream)
//! - Handles SSE responses from POST requests, resuming them if the connection
//!   drops before the response arrives
//! - Auto-reconnect with exponential backoff, honouring the server's `retry`
//! - Per-stream Last-Event-ID resumability
//! - Session management with Mcp-Session-Id, including the 404 that ends one
//! - Protocol version headers carrying the negotiated version
//!
//! This is the Streamable HTTP transport only; it does not fall back to the
//! 2024-11-05 HTTP+SSE transport. An `endpoint` event arriving on a stream is
//! honoured only when it names the MCP endpoint's own origin.

use bytes::Bytes;
use futures::StreamExt;
use reqwest::{Client as HttpClient, header};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock, mpsc};
use tracing::{debug, error, info, warn};
use url::Url;

use turbomcp_protocol::MessageId;
use turbomcp_transport_traits::{
    AtomicMetrics, LimitsConfig, TlsConfig, TlsVersion, Transport, TransportCapabilities,
    TransportError, TransportEventEmitter, TransportMessage, TransportMetrics, TransportResult,
    TransportState, TransportType, validate_request_size, validate_response_size,
};

/// Normalize SSE line endings to bare `\n`.
///
/// The SSE specification (and the MCP servers built on frameworks that follow it) permits a line
/// to be terminated by `\r\n`, a lone `\r`, or `\n` — clients are required to accept all three.
/// Both SSE read loops in this file locate event boundaries with a `\n\n` search, which silently
/// finds nothing (and therefore silently drops every event, with no error) against a server that
/// emits `\r\n`. Confirmed live against a real MCP server that does exactly this.
///
/// Applied per-chunk before appending to the read buffer. A chunk boundary that happens to land
/// exactly inside a `\r\n` pair produces one extra blank line in the reassembled buffer in the
/// rare worst case — harmless, since the per-field event parser below already treats blank lines
/// as a no-op — so this stays a simple per-chunk pass rather than carrying a pending-CR byte
/// across chunk boundaries.
fn normalize_sse_line_endings(chunk: &str) -> std::borrow::Cow<'_, str> {
    if chunk.contains('\r') {
        std::borrow::Cow::Owned(chunk.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        std::borrow::Cow::Borrowed(chunk)
    }
}

/// The fields of one SSE event that MCP uses.
#[derive(Debug, Default, PartialEq)]
struct SseEvent {
    event: Option<String>,
    /// `data` lines joined with `\n`; `None` when the event had none.
    data: Option<String>,
    /// The event's id. `Some("")` is an explicit reset of the stream's cursor.
    id: Option<String>,
    /// The server's reconnection delay.
    retry: Option<Duration>,
}

/// Parse one SSE event, already split off its blank-line boundary.
///
/// Field handling follows the WHATWG event-stream rules: comment lines are
/// skipped, one space after the colon is dropped, an `id` containing NUL is
/// ignored, and `retry` counts only when it is all ASCII digits.
fn parse_sse_event(event_str: &str) -> SseEvent {
    let mut event = SseEvent::default();
    let mut data: Vec<&str> = Vec::new();

    for line in event_str.lines() {
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "event" => event.event = Some(value.to_string()),
            "data" => data.push(value),
            "id" if !value.contains('\0') => event.id = Some(value.to_string()),
            "retry" if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) => {
                if let Ok(millis) = value.parse() {
                    event.retry = Some(Duration::from_millis(millis));
                }
            }
            _ => {}
        }
    }

    if !data.is_empty() {
        event.data = Some(data.join("\n"));
    }
    event
}

/// Record an event's `id` as its stream's resumption cursor.
fn advance_cursor(cursor: &mut Option<String>, event: &SseEvent) {
    if let Some(id) = &event.id {
        *cursor = (!id.is_empty()).then(|| id.clone());
    }
}

/// How long to wait before reconnecting a stream, if at all.
///
/// §Sending Messages item 6: "The client MUST respect the `retry` field,
/// waiting the given number of milliseconds before attempting to reconnect."
/// The server's figure is a floor rather than a replacement for backoff, so a
/// server that keeps failing still earns growing delays.
fn reconnect_delay(backoff: Option<Duration>, server_retry: Option<Duration>) -> Option<Duration> {
    match (backoff, server_retry) {
        (Some(backoff), Some(retry)) => Some(backoff.max(retry)),
        (backoff, retry) => backoff.or(retry),
    }
}

/// Resolve a legacy `endpoint` event against the MCP endpoint.
///
/// The event redirects every later POST, `Authorization` header and all, so
/// one naming another origin is refused: honouring it would hand the client's
/// credentials to whoever could put an event on the stream. The data may be a
/// bare URI or `{"uri": "..."}`, and a relative URI resolves against the MCP
/// endpoint.
fn resolve_endpoint_event(endpoint_url: &str, data: &str) -> TransportResult<String> {
    let uri = if data.trim_start().starts_with('{') {
        let value: serde_json::Value = serde_json::from_str(data).map_err(|e| {
            TransportError::SerializationFailed(format!("Invalid endpoint JSON: {e}"))
        })?;
        value["uri"]
            .as_str()
            .ok_or_else(|| {
                TransportError::SerializationFailed(
                    "Endpoint event missing 'uri' field".to_string(),
                )
            })?
            .to_string()
    } else {
        data.trim().to_string()
    };

    let base = Url::parse(endpoint_url)
        .map_err(|e| TransportError::ConfigurationError(format!("Invalid MCP endpoint: {e}")))?;
    let resolved = base
        .join(&uri)
        .map_err(|e| TransportError::ProtocolError(format!("Invalid endpoint URI {uri:?}: {e}")))?;
    if resolved.origin() != base.origin() {
        return Err(TransportError::ProtocolError(format!(
            "Refusing endpoint event naming another origin: {resolved}"
        )));
    }
    Ok(resolved.to_string())
}

/// Why a POST's SSE stream stopped being read.
#[derive(Debug, PartialEq)]
enum PostStreamEnd {
    /// The response to the POST arrived.
    Answered,
    /// The connection ended first.
    Interrupted,
}

/// Retry policy for auto-reconnect
#[derive(Clone, Debug)]
pub enum RetryPolicy {
    /// Fixed interval between retries
    Fixed {
        /// Time interval between retry attempts
        interval: Duration,
        /// Maximum number of retry attempts (None for unlimited)
        max_attempts: Option<u32>,
    },
    /// Exponential backoff
    Exponential {
        /// Base delay for exponential backoff calculation
        base: Duration,
        /// Maximum delay between retry attempts
        max_delay: Duration,
        /// Maximum number of retry attempts (None for unlimited)
        max_attempts: Option<u32>,
    },
    /// Never retry
    Never,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::Exponential {
            base: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            max_attempts: Some(10),
        }
    }
}

/// How long a standalone SSE stream must stay up before it counts as a *successful* connection
/// rather than a failed one.
///
/// Connecting is not the same as working. A server that accepts the GET and then immediately closes
/// the stream — no keepalive, or it does not really support the standalone channel — used to reset
/// the backoff counter on every accept, so the client reconnected with **zero** delay, forever. In
/// the field that produced ~50 reconnects a minute on a completely idle connection (93.5k in 24h),
/// which survives functionally but buries every other diagnostic in the log.
///
/// Streams that do real work run far longer than this, so a genuine reconnect after a deploy still
/// gets a full set of fresh attempts.
/// Default for [`StreamableHttpClientConfig::sse_healthy_stream_threshold`].
///
/// Well below a typical server idle timeout on purpose. Servers commonly close an idle SSE stream
/// on a round number — 30s and 60s are both common — and a threshold at or above that would
/// classify every ordinary cycle as a failure, accumulate backoff against normal operation, and
/// eventually abandon the channel. Measured against one such server: streams ended at 29.9999s,
/// every time.
pub const DEFAULT_SSE_HEALTHY_STREAM_THRESHOLD: Duration = Duration::from_secs(10);

/// Whether a stream that stayed up for `uptime` did real work.
///
/// The single judgement this file makes about a closed stream, used twice: to decide whether the
/// backoff counter resets, and to decide whether the reconnect is worth logging loudly. A server
/// that closes idle streams on a timer is behaving normally and should be neither backed off from
/// nor reported as an error.
fn stream_was_healthy(uptime: Duration, threshold: Duration) -> bool {
    uptime >= threshold
}

/// The attempt counter after a stream ends, given how long it was up.
///
/// Separated from the loop so the rule is stated once and can be tested without a server: a stream
/// that lasted is a success and clears the backoff; one that collapsed immediately is a failure and
/// must count as one, or backoff never engages.
fn next_attempt_after_stream_end(previous: u32, uptime: Duration, threshold: Duration) -> u32 {
    if stream_was_healthy(uptime, threshold) {
        0
    } else {
        previous.saturating_add(1)
    }
}

impl RetryPolicy {
    pub(crate) fn delay(&self, attempt: u32) -> Option<Duration> {
        match self {
            Self::Fixed {
                interval,
                max_attempts,
            } => {
                if let Some(max) = max_attempts
                    && attempt >= *max
                {
                    return None;
                }
                Some(*interval)
            }
            Self::Exponential {
                base,
                max_delay,
                max_attempts,
            } => {
                if let Some(max) = max_attempts
                    && attempt >= *max
                {
                    return None;
                }
                let base_delay = base.as_millis() as u64 * 2u64.pow(attempt);
                let max_delay_ms = max_delay.as_millis() as u64;
                let capped = base_delay.min(max_delay_ms);
                // Add ±25% jitter to prevent thundering herd. Sourced per-instance
                // from `fastrand` so concurrent clients on the same attempt number
                // do not produce identical delays.
                let jitter_range = capped / 4;
                let jitter_offset = if jitter_range > 0 {
                    fastrand::u64(0..jitter_range * 2)
                } else {
                    0
                };
                let final_delay = capped
                    .saturating_sub(jitter_range)
                    .saturating_add(jitter_offset);
                Some(Duration::from_millis(final_delay))
            }
            Self::Never => None,
        }
    }
}

/// Streamable HTTP client configuration
#[derive(Clone, Debug)]
pub struct StreamableHttpClientConfig {
    /// Base URL (e.g., <https://api.example.com>)
    pub base_url: String,

    /// MCP endpoint path (e.g., "/mcp")
    pub endpoint_path: String,

    /// Request timeout.
    ///
    /// Bounds connecting, a request up to its response headers, and a JSON
    /// response body. It does not bound an SSE stream, which can rightly stay
    /// open far longer — a POST streaming a slow tool call, or the standalone
    /// GET stream — and is guarded by [`Self::sse_read_timeout`] instead.
    pub timeout: Duration,

    /// Auto-reconnect policy
    pub retry_policy: RetryPolicy,

    /// Authentication token
    pub auth_token: Option<String>,

    /// Custom headers
    pub headers: HashMap<String, String>,

    /// User agent string (set to None to disable User-Agent header)
    ///
    /// Default: `TurboMCP-Client/{version}`
    ///
    /// # Security Note
    ///
    /// The User-Agent header can expose client version information. Consider:
    /// - Setting to `None` to disable User-Agent header entirely
    /// - Using a generic string like "MCP-Client" to minimize fingerprinting
    /// - Keeping the default to aid server-side debugging and analytics
    pub user_agent: Option<String>,

    /// Protocol version to use
    pub protocol_version: String,

    /// Size limits for requests and responses (v2.2.0+)
    pub limits: LimitsConfig,

    /// TLS/HTTPS configuration (v2.2.0+)
    pub tls: TlsConfig,

    /// Idle timeout between SSE chunks.
    ///
    /// Guards against a silent TCP half-open where the server stops writing
    /// without closing the connection. If no chunk arrives within this window,
    /// the SSE task breaks and the reconnect loop takes over. Set generously —
    /// the SSE protocol tolerates long idle periods between events. Default: 5 minutes.
    pub sse_read_timeout: Duration,

    /// How long a standalone SSE stream must stay up to count as having done real work.
    ///
    /// Below this, a stream that ends is treated as a failed attempt: backoff accrues and the
    /// reconnect is logged as a warning. At or above it, the stream is considered to have worked —
    /// the counter resets and the reconnect is routine (`debug`).
    ///
    /// Keep it comfortably under the server's idle timeout. See
    /// [`DEFAULT_SSE_HEALTHY_STREAM_THRESHOLD`].
    pub sse_healthy_stream_threshold: Duration,
}

impl Default for StreamableHttpClientConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:8080".to_string(),
            endpoint_path: "/mcp".to_string(),
            timeout: Duration::from_secs(30),
            retry_policy: RetryPolicy::default(),
            auth_token: None,
            headers: HashMap::new(),
            user_agent: Some(format!("TurboMCP-Client/{}", env!("CARGO_PKG_VERSION"))),
            protocol_version: "2025-11-25".to_string(),
            limits: LimitsConfig::default(),
            tls: TlsConfig::default(),
            sse_read_timeout: Duration::from_secs(300),
            sse_healthy_stream_threshold: DEFAULT_SSE_HEALTHY_STREAM_THRESHOLD,
        }
    }
}

/// What every request to the MCP endpoint carries for the current session.
///
/// POST, GET and DELETE all build their headers here, so none can drift from
/// the others. They used to assemble their own: the GET sent the configured
/// protocol version rather than the negotiated one, and DELETE sent no
/// version, no credentials and no custom headers at all.
#[derive(Clone)]
struct SessionState {
    config: Arc<StreamableHttpClientConfig>,

    /// Session ID from server
    session_id: Arc<RwLock<Option<String>>>,

    /// Protocol version actually negotiated with this server.
    ///
    /// The transport spec requires post-initialize requests to carry the
    /// version "negotiated during initialization", not a compile-time guess.
    /// `None` until the `initialize` response is seen, at which point the
    /// configured default stops being used.
    negotiated_version: Arc<RwLock<Option<String>>>,
}

impl SessionState {
    fn new(config: Arc<StreamableHttpClientConfig>) -> Self {
        Self {
            config,
            session_id: Arc::new(RwLock::new(None)),
            negotiated_version: Arc::new(RwLock::new(None)),
        }
    }

    /// Headers for a request to the MCP endpoint.
    ///
    /// Never `Last-Event-ID`: that names one stream's position, so only the
    /// GET resuming that stream may send it.
    async fn headers(&self, accept: Option<&str>) -> header::HeaderMap {
        let mut headers = header::HeaderMap::new();

        // Use safe header value construction - skip invalid headers rather than panic
        if let Some(accept) = accept
            && let Ok(accept_value) = header::HeaderValue::from_str(accept)
        {
            headers.insert(header::ACCEPT, accept_value);
        }

        // Prefer what was actually negotiated; the configured value is only a
        // pre-handshake default. Sending a version the server never agreed to
        // invites a 400 from any server that validates the header.
        let protocol_version = self
            .negotiated_version
            .read()
            .await
            .clone()
            .unwrap_or_else(|| self.config.protocol_version.clone());
        if let Ok(protocol_value) = header::HeaderValue::from_str(&protocol_version) {
            headers.insert("MCP-Protocol-Version", protocol_value);
        }

        if let Some(session_id) = self.session_id.read().await.as_ref()
            && let Ok(session_value) = header::HeaderValue::from_str(session_id)
        {
            headers.insert("Mcp-Session-Id", session_value);
        }

        if let Some(token) = &self.config.auth_token
            && let Ok(auth_value) = header::HeaderValue::from_str(&format!("Bearer {}", token))
        {
            headers.insert(header::AUTHORIZATION, auth_value);
        }

        for (key, value) in &self.config.headers {
            if let (Ok(k), Ok(v)) = (
                header::HeaderName::from_bytes(key.as_bytes()),
                header::HeaderValue::from_str(value),
            ) {
                headers.insert(k, v);
            }
        }

        headers
    }

    /// Record the protocol version from an `initialize` response.
    ///
    /// The server may answer with a version other than the one requested —
    /// that is the negotiation the lifecycle spec prescribes — and every later
    /// request has to carry *that* version in `MCP-Protocol-Version`. Anything
    /// that is not an initialize result is ignored, so this is safe to call on
    /// every response body.
    async fn capture_negotiated_version(&self, body: &[u8]) {
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
            return;
        };
        if let Some(version) = value
            .get("result")
            .and_then(|r| r.get("protocolVersion"))
            .and_then(|v| v.as_str())
        {
            let mut negotiated = self.negotiated_version.write().await;
            if negotiated.as_deref() != Some(version) {
                debug!("Negotiated MCP protocol version: {version}");
                *negotiated = Some(version.to_string());
            }
        }
    }

    /// Forget the session, and what was negotiated for it.
    async fn reset(&self) {
        *self.session_id.write().await = None;
        *self.negotiated_version.write().await = None;
    }
}

/// Streamable HTTP client transport
pub struct StreamableHttpClientTransport {
    config: Arc<StreamableHttpClientConfig>,
    http_client: HttpClient,
    state: Arc<RwLock<TransportState>>,
    capabilities: TransportCapabilities,
    /// Lock-free metrics counters — updated on every message send/receive,
    /// so this must not sit behind a lock (see `turbomcp-stdio` for the
    /// same pattern).
    metrics: Arc<AtomicMetrics>,
    _event_emitter: TransportEventEmitter,

    /// Message endpoint named by a same-origin `endpoint` event, if a server
    /// sent one.
    ///
    /// MCP 2025-11-25 Streamable HTTP uses a single MCP endpoint for POST and
    /// GET; the `endpoint` event belongs to the older HTTP+SSE transport. See
    /// [`resolve_endpoint_event`] for why only the endpoint's own origin is
    /// accepted.
    message_endpoint: Arc<RwLock<Option<String>>>,

    /// Session id, negotiated version, and the headers built from them.
    session: SessionState,

    /// Channel for incoming SSE messages
    sse_receiver: Arc<Mutex<mpsc::Receiver<TransportMessage>>>,
    sse_sender: mpsc::Sender<TransportMessage>,

    /// Channel for immediate JSON responses from POST requests
    response_receiver: Arc<Mutex<mpsc::Receiver<TransportMessage>>>,
    response_sender: mpsc::Sender<TransportMessage>,

    /// SSE connection task handle
    sse_task_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl std::fmt::Debug for StreamableHttpClientTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamableHttpClientTransport")
            .field("base_url", &self.config.base_url)
            .field("endpoint", &self.config.endpoint_path)
            .finish()
    }
}

impl StreamableHttpClientTransport {
    /// Create a new streamable HTTP client transport.
    ///
    /// Returns an error if the underlying HTTP client cannot be built — most often a
    /// bad TLS configuration (e.g., custom CA certificates that won't load against the
    /// platform verifier). Pre-3.1 this was an `expect` and would panic the calling
    /// process; v3.1 propagates it instead.
    pub fn new(config: StreamableHttpClientConfig) -> TransportResult<Self> {
        let (sse_tx, sse_rx) = mpsc::channel(1000);
        let (response_tx, response_rx) = mpsc::channel(100);
        let (event_emitter, _) = TransportEventEmitter::new();

        // Emit insecurity warning if certificate validation is disabled
        if config.tls.is_insecure() {
            warn!(
                "Certificate validation is disabled. This is insecure and should only be used \
                 for testing or in secure mTLS mesh environments. \
                 See https://turbomcp.org/docs/security/tls#certificate-validation"
            );
        }

        // Build HTTP client with TLS configuration
        // IMPORTANT: Must explicitly call use_rustls_tls() because cargo features are additive
        // and other dependencies may bring in native-tls. Without this, TLS 1.3 minimum fails.
        // See: https://github.com/seanmonstar/reqwest/issues/1314
        //
        // No client-wide `timeout`: reqwest applies it until the body is fully read, which cut
        // every SSE stream off after `config.timeout` — a slow tool call's POST stream included,
        // so its response never arrived. Each request bounds its own non-streaming phases.
        let mut client_builder = HttpClient::builder()
            .use_rustls_tls()
            .connect_timeout(config.timeout);

        // Redirect policy: when carrying a bearer token, only follow same-origin redirects
        // so the `Authorization: Bearer …` header (preserved by reqwest across redirects)
        // cannot leak to a third-party host. Without an auth token we keep the default
        // redirect behaviour (up to 10 follows) for compatibility with bog-standard HTTP.
        if config.auth_token.is_some() {
            client_builder =
                client_builder.redirect(reqwest::redirect::Policy::custom(|attempt| {
                    if attempt.previous().len() >= 10 {
                        return attempt.error("too many redirects");
                    }
                    let prev_origin = attempt.previous().last().map(reqwest::Url::origin);
                    if prev_origin.as_ref() == Some(&attempt.url().origin()) {
                        attempt.follow()
                    } else {
                        // Stop the redirect chain; surface a 3xx to the caller so they can
                        // re-authenticate against the new origin if appropriate.
                        attempt.stop()
                    }
                }));
        }

        // Set User-Agent header if configured
        if let Some(ref user_agent) = config.user_agent {
            client_builder = client_builder.user_agent(user_agent);
        }

        // Configure TLS version (TLS 1.3 only in v3.0)
        client_builder = match config.tls.min_version {
            TlsVersion::Tls13 => client_builder.min_tls_version(reqwest::tls::Version::TLS_1_3),
        };

        // Configure certificate validation with security gate
        if !config.tls.validate_certificates {
            // SECURITY: Require explicit env var opt-in for insecure TLS
            // This prevents accidental deployment of insecure configurations
            const INSECURE_TLS_ENV_VAR: &str = "TURBOMCP_ALLOW_INSECURE_TLS";

            if std::env::var(INSECURE_TLS_ENV_VAR).is_err() {
                error!(
                    "SECURITY: Certificate validation disabled but {} not set. \
                     Overriding to validate_certificates=true for safety. \
                     Set {}=1 to allow insecure TLS.",
                    INSECURE_TLS_ENV_VAR, INSECURE_TLS_ENV_VAR
                );
                // Override: force secure config instead of panicking
                // Don't apply danger_accept_invalid_certs
            } else {
                warn!(
                    "SECURITY WARNING: TLS certificate validation is DISABLED. \
                     This configuration is INSECURE and should ONLY be used: \
                     (1) In development/testing environments, or \
                     (2) In secure mTLS mesh where validation happens elsewhere. \
                     NEVER use in production connecting to untrusted servers."
                );

                client_builder = client_builder.danger_accept_invalid_certs(true);
            }
        }

        // Add custom CA certificates if provided
        if let Some(ca_certs) = &config.tls.custom_ca_certs {
            let mut loaded = 0usize;
            let total = ca_certs.len();
            for cert_bytes in ca_certs {
                // Try to parse as PEM or DER
                if let Ok(cert) = reqwest::Certificate::from_pem(cert_bytes) {
                    client_builder = client_builder.add_root_certificate(cert);
                    loaded += 1;
                } else if let Ok(cert) = reqwest::Certificate::from_der(cert_bytes) {
                    client_builder = client_builder.add_root_certificate(cert);
                    loaded += 1;
                } else {
                    warn!(
                        "Failed to parse custom CA certificate ({}/{}), skipping",
                        loaded + 1,
                        total
                    );
                }
            }
            if loaded == 0 && total > 0 {
                error!("All {} custom CA certificates failed to parse", total);
                // Don't panic - but log at error level. The connection will likely fail with TLS errors.
            }
            if loaded > 0 {
                info!("Loaded {}/{} custom CA certificates", loaded, total);
            }
        }

        let http_client = client_builder.build().map_err(|e| {
            TransportError::ConfigurationError(format!(
                "Failed to build HTTP client (likely bad TLS configuration): {e}"
            ))
        })?;

        let config = Arc::new(config);
        Ok(Self {
            session: SessionState::new(Arc::clone(&config)),
            config,
            http_client,
            state: Arc::new(RwLock::new(TransportState::Disconnected)),
            capabilities: TransportCapabilities {
                max_message_size: Some(turbomcp_protocol::MAX_MESSAGE_SIZE),
                supports_compression: false,
                supports_streaming: true,
                supports_bidirectional: true,
                supports_multiplexing: false,
                compression_algorithms: Vec::new(),
                custom: HashMap::new(),
            },
            metrics: Arc::new(AtomicMetrics::default()),
            _event_emitter: event_emitter,
            message_endpoint: Arc::new(RwLock::new(None)),
            sse_receiver: Arc::new(Mutex::new(sse_rx)),
            sse_sender: sse_tx,
            response_receiver: Arc::new(Mutex::new(response_rx)),
            response_sender: response_tx,
            sse_task_handle: Arc::new(Mutex::new(None)),
        })
    }

    #[allow(dead_code)]
    fn record_message_sent_inner(&self, payload_len: usize) {
        self.metrics.messages_sent.fetch_add(1, Ordering::Relaxed);
        self.metrics
            .bytes_sent
            .fetch_add(payload_len as u64, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    fn record_message_received_inner(&self, payload_len: usize) {
        self.metrics
            .messages_received
            .fetch_add(1, Ordering::Relaxed);
        self.metrics
            .bytes_received
            .fetch_add(payload_len as u64, Ordering::Relaxed);
    }

    /// Record one sent message in the transport metrics.
    ///
    /// Exposed under the `internal-bench` feature for `benches/metrics_recording.rs`;
    /// not part of the supported public API.
    #[cfg(not(feature = "internal-bench"))]
    pub(crate) fn record_message_sent(&self, payload_len: usize) {
        self.record_message_sent_inner(payload_len);
    }

    #[cfg(feature = "internal-bench")]
    #[doc(hidden)]
    pub fn record_message_sent(&self, payload_len: usize) {
        self.record_message_sent_inner(payload_len);
    }

    /// Record one received message in the transport metrics.
    ///
    /// Exposed under the `internal-bench` feature for `benches/metrics_recording.rs`;
    /// not part of the supported public API.
    #[cfg(not(feature = "internal-bench"))]
    pub(crate) fn record_message_received(&self, payload_len: usize) {
        self.record_message_received_inner(payload_len);
    }

    #[cfg(feature = "internal-bench")]
    #[doc(hidden)]
    pub fn record_message_received(&self, payload_len: usize) {
        self.record_message_received_inner(payload_len);
    }

    /// Get full endpoint URL
    fn get_endpoint_url(&self) -> String {
        format!("{}{}", self.config.base_url, self.config.endpoint_path)
    }

    /// Get message endpoint URL (discovered or default)
    async fn get_message_endpoint_url(&self) -> String {
        self.message_endpoint
            .read()
            .await
            .clone()
            .unwrap_or_else(|| self.get_endpoint_url())
    }

    /// Forget a session the server has terminated, and say so.
    ///
    /// §Session Management: a client that gets 404 for a request carrying
    /// `Mcp-Session-Id` "MUST start a new session by sending a new
    /// `InitializeRequest` without a session ID attached". Everything tied to
    /// the old session goes: its id, the version negotiated for it, and the
    /// standalone stream still polling it — left running, that stream would
    /// go on reconnecting with the dead id. Keeping the id would wedge the
    /// transport outright: every later POST would resend it and get another
    /// 404.
    async fn expire_session(&self) -> TransportError {
        self.session.reset().await;
        *self.message_endpoint.write().await = None;
        if let Some(handle) = self.sse_task_handle.lock().await.take() {
            handle.abort();
        }
        TransportError::SessionExpired(
            "the server no longer knows this session (HTTP 404); initialize again to start a new one"
                .to_string(),
        )
    }

    /// Start SSE connection task
    async fn start_sse_connection(&self) -> TransportResult<()> {
        if self.session.session_id.read().await.is_none() {
            debug!("Deferring SSE connection until server provides a session ID");
            return Ok(());
        }

        let mut task_handle = self.sse_task_handle.lock().await;
        if let Some(handle) = task_handle.as_ref()
            && !handle.is_finished()
        {
            debug!("SSE connection task already running");
            return Ok(());
        }

        info!("Starting SSE connection to {}", self.get_endpoint_url());

        let task = tokio::spawn(Self::sse_connection_task(
            self.get_endpoint_url(),
            self.http_client.clone(),
            Arc::clone(&self.state),
            self.sse_sender.clone(),
            self.session.clone(),
            Arc::clone(&self.message_endpoint),
        ));

        *task_handle = Some(task);

        Ok(())
    }

    /// SSE connection task with auto-reconnect
    async fn sse_connection_task(
        endpoint_url: String,
        http_client: HttpClient,
        state: Arc<RwLock<TransportState>>,
        sse_sender: mpsc::Sender<TransportMessage>,
        session: SessionState,
        message_endpoint: Arc<RwLock<Option<String>>>,
    ) {
        let config = Arc::clone(&session.config);
        let mut attempt = 0u32;
        // This stream's own resumption cursor. A POST's stream keeps its own:
        // one cursor shared between them resumed each stream from the other's
        // position, which §Resumability forbids ("MUST NOT replay messages that
        // would have been delivered on a different stream").
        let mut last_event_id: Option<String> = None;
        let mut server_retry: Option<Duration> = None;

        loop {
            // Check if we should retry
            let Some(backoff) = config.retry_policy.delay(attempt) else {
                error!("Max retry attempts reached, giving up");
                *state.write().await = TransportState::Disconnected;
                break;
            };
            if let Some(delay) = reconnect_delay((attempt > 0).then_some(backoff), server_retry) {
                if attempt > 0 {
                    warn!("Reconnecting in {:?} (attempt {})", delay, attempt + 1);
                } else {
                    debug!("Reconnecting in {:?}, as the server asked", delay);
                }
                tokio::time::sleep(delay).await;
            }

            let mut headers = session.headers(Some("text/event-stream")).await;
            if let Some(last_id) = last_event_id.as_deref()
                && let Ok(event_value) = header::HeaderValue::from_str(last_id)
            {
                headers.insert("Last-Event-ID", event_value);
            }

            // Connect to SSE endpoint. Only the wait for headers is bounded
            // here; the body is an open-ended stream, guarded per chunk below.
            let request = http_client.get(&endpoint_url).headers(headers).send();
            let response = match tokio::time::timeout(config.timeout, request).await {
                Ok(Ok(response)) => response,
                Ok(Err(e)) => {
                    error!("Failed to connect: {}", e);
                    attempt += 1;
                    continue;
                }
                Err(_) => {
                    error!("SSE connection timed out after {:?}", config.timeout);
                    attempt += 1;
                    continue;
                }
            };

            if response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
                info!(
                    "Server returned HTTP 405 for GET {}. Continuing without standalone SSE polling.",
                    endpoint_url
                );
                break;
            }

            // The session is gone. Reconnecting with its id can only earn more
            // 404s; the next POST surfaces the expiry to the caller.
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                info!("Session no longer exists; stopping standalone SSE stream");
                break;
            }

            if !response.status().is_success() {
                error!("SSE connection failed: {}", response.status());
                attempt += 1;
                continue;
            }

            info!("SSE connection established");
            *state.write().await = TransportState::Connected;
            // Deliberately *not* resetting `attempt` here: accepting the GET is not
            // evidence the stream works. That is decided below, from how long it lasted.
            let connected_at = Instant::now();

            // Process SSE stream
            let mut stream = response.bytes_stream();
            let mut buffer = String::new();
            let read_timeout = config.sse_read_timeout;
            // Cap a single SSE event's accumulated buffer at the response-size limit so
            // a server that streams indefinitely without ever emitting `\n\n` cannot
            // OOM the client. `None` keeps the historical "no cap" behaviour.
            let buffer_cap = config
                .limits
                .enforce_on_streams
                .then_some(config.limits.max_response_size)
                .flatten();

            'sse_loop: loop {
                let chunk_result = match tokio::time::timeout(read_timeout, stream.next()).await {
                    Ok(Some(r)) => r,
                    Ok(None) => break,
                    Err(_) => {
                        warn!(
                            "SSE read idle for {:?}; closing stream to reconnect",
                            read_timeout
                        );
                        break;
                    }
                };
                match chunk_result {
                    Ok(chunk) => {
                        let chunk_str = String::from_utf8_lossy(&chunk);
                        // The SSE spec (and MCP servers built on frameworks that default
                        // to it) permits `\r\n` or a lone `\r` as a line terminator, not
                        // just `\n` — normalize per chunk so the `\n\n` event-boundary
                        // search below works regardless of which convention the server
                        // uses. (A chunk boundary landing exactly inside a `\r\n` pair
                        // yields one extra blank line in the rare worst case, which the
                        // per-field parser below already treats as a no-op — not worth
                        // the complexity of carrying a pending-CR byte across chunks.)
                        buffer.push_str(&normalize_sse_line_endings(&chunk_str));

                        // Process complete events
                        while let Some(pos) = buffer.find("\n\n") {
                            let event = parse_sse_event(&buffer[..pos]);
                            buffer.drain(..pos + 2);

                            advance_cursor(&mut last_event_id, &event);
                            if event.retry.is_some() {
                                server_retry = event.retry;
                            }
                            if let Err(e) = Self::process_sse_event(
                                event,
                                &sse_sender,
                                &message_endpoint,
                                &endpoint_url,
                            )
                            .await
                            {
                                warn!("Failed to process SSE event: {}", e);
                            }
                        }

                        if let Some(cap) = buffer_cap
                            && buffer.len() > cap
                        {
                            error!(
                                "SSE event buffer exceeded {} bytes without an event \
                                 boundary; closing stream to avoid OOM",
                                cap
                            );
                            break 'sse_loop;
                        }
                    }
                    Err(e) => {
                        // Not necessarily a fault: a server closing an idle stream
                        // surfaces here as a decode error. Whether that mattered is
                        // decided below, from how long the stream lasted — logging it as
                        // an error unconditionally reports normal operation as a failure.
                        if stream_was_healthy(
                            connected_at.elapsed(),
                            config.sse_healthy_stream_threshold,
                        ) {
                            debug!("SSE stream closed by server: {}", e);
                        } else {
                            error!("Error reading SSE stream: {}", e);
                        }
                        break;
                    }
                }
            }

            let uptime = connected_at.elapsed();
            let healthy = stream_was_healthy(uptime, config.sse_healthy_stream_threshold);
            attempt =
                next_attempt_after_stream_end(attempt, uptime, config.sse_healthy_stream_threshold);
            if healthy {
                // A server that closes idle streams on a timer is behaving normally, and
                // this is the client doing its job. Reporting it at warn/error once per
                // cycle per peer is what buried real diagnostics under log rotation.
                debug!("SSE stream ended after {:?}; reconnecting", uptime);
            } else {
                warn!(
                    "SSE stream ended after only {:?} (attempt {}); backing off",
                    uptime, attempt
                );
            }
            *state.write().await = TransportState::Disconnected;
        }
    }

    /// Process an SSE event from the standalone GET stream.
    async fn process_sse_event(
        event: SseEvent,
        sse_sender: &mpsc::Sender<TransportMessage>,
        message_endpoint: &Arc<RwLock<Option<String>>>,
        endpoint_url: &str,
    ) -> TransportResult<()> {
        let Some(data_str) = event.data else {
            return Ok(());
        };

        // Handle different event types
        match event.event.as_deref() {
            Some("endpoint") => {
                // Legacy HTTP+SSE transport compatibility. Streamable HTTP
                // (MCP 2025-11-25) uses a single endpoint, so connect/send must not
                // depend on this event.
                let endpoint = resolve_endpoint_event(endpoint_url, &data_str)?;
                info!("Discovered message endpoint: {}", endpoint);
                *message_endpoint.write().await = Some(endpoint);
                Ok(())
            }
            Some("message") | None => {
                // Skip empty or whitespace-only events (keep-alive, malformed events)
                // This is defensive against server sending empty data events
                if data_str.trim().is_empty() {
                    debug!("Skipping empty SSE event");
                    return Ok(());
                }

                // Parse as JSON-RPC message
                let json_value: serde_json::Value =
                    serde_json::from_str(&data_str).map_err(|e| {
                        TransportError::SerializationFailed(format!("Invalid JSON: {}", e))
                    })?;

                let message = TransportMessage::new(
                    MessageId::from("sse-message".to_string()),
                    Bytes::from(
                        serde_json::to_vec(&json_value)
                            .map_err(|e| TransportError::SerializationFailed(e.to_string()))?,
                    ),
                );

                sse_sender
                    .send(message)
                    .await
                    .map_err(|e| TransportError::ConnectionLost(e.to_string()))?;

                debug!("Received SSE message");
                Ok(())
            }
            Some(other) => {
                debug!("Ignoring unknown event type: {}", other);
                Ok(())
            }
        }
    }

    /// Queue one SSE event from a POST response stream, and report whether it
    /// was the JSON-RPC *response* correlated to `expected_id` — as opposed to
    /// some other message (a request or notification) the server chose to send
    /// first over the same stream.
    ///
    /// Per the MCP Streamable HTTP transport, a server MAY keep this per-POST SSE stream open
    /// after sending the correlated response (e.g. to send further related messages later); the
    /// caller must stop reading once it has that response rather than waiting for the stream to
    /// close, which is not guaranteed to happen. See `send()`'s call site.
    async fn process_post_sse_event(
        event: &SseEvent,
        response_sender: &mpsc::Sender<TransportMessage>,
        expected_id: Option<&serde_json::Value>,
    ) -> TransportResult<bool> {
        let Some(data_str) = event.data.as_deref() else {
            return Ok(false);
        };
        if data_str.trim().is_empty() {
            debug!("Skipping empty POST SSE event");
            return Ok(false);
        }

        // Parse as JSON-RPC message
        let json_value: serde_json::Value = serde_json::from_str(data_str).map_err(|e| {
            TransportError::SerializationFailed(format!("Invalid JSON in POST SSE: {}", e))
        })?;

        // A JSON-RPC *response* carries "id" plus "result" xor "error" — that's what the caller
        // is waiting for. A request or notification the server sends first over the same stream
        // (e.g. a progress notification) has no such shape and must not end the read loop early.
        let is_correlated_response = json_value.get("id").is_some()
            && (json_value.get("result").is_some() || json_value.get("error").is_some())
            && match expected_id {
                Some(expected) => json_value.get("id") == Some(expected),
                None => true,
            };

        let message = TransportMessage::new(
            MessageId::from("post-sse-response".to_string()),
            Bytes::from(
                serde_json::to_vec(&json_value)
                    .map_err(|e| TransportError::SerializationFailed(e.to_string()))?,
            ),
        );

        response_sender
            .send(message.clone())
            .await
            .map_err(|e| TransportError::ConnectionLost(e.to_string()))?;

        debug!(
            "Queued message from POST SSE stream: {}",
            String::from_utf8_lossy(&message.payload)
        );
        Ok(is_correlated_response)
    }

    /// Read a POST's SSE answer until the response to `expected_id` arrives,
    /// resuming the stream if its connection ends first.
    ///
    /// Returning before then would leave the caller waiting on a response that
    /// nothing is going to deliver: this stream is the only place the server
    /// puts it. §Sending Messages item 6 lets a connection end at any time
    /// without ending the stream, and §Resumability says how to carry on — a
    /// GET naming this stream's own last event id. A stream that ended without
    /// ever carrying an id cannot be resumed, and is reported as lost.
    async fn read_post_stream(
        &self,
        response: reqwest::Response,
        expected_id: Option<&serde_json::Value>,
    ) -> TransportResult<()> {
        let mut response = Some(response);
        let mut cursor: Option<String> = None;
        let mut server_retry: Option<Duration> = None;
        let mut attempt = 0u32;

        loop {
            if let Some(response) = response.take()
                && self
                    .drain_post_stream(response, expected_id, &mut cursor, &mut server_retry)
                    .await?
                    == PostStreamEnd::Answered
            {
                return Ok(());
            }

            let Some(last_event_id) = cursor.clone() else {
                return Err(TransportError::ConnectionLost(
                    "POST SSE stream ended before its response, with no event id to resume from"
                        .to_string(),
                ));
            };
            let Some(backoff) = self.config.retry_policy.delay(attempt) else {
                return Err(TransportError::ConnectionLost(
                    "POST SSE stream ended before its response; gave up resuming it".to_string(),
                ));
            };
            if let Some(delay) = reconnect_delay((attempt > 0).then_some(backoff), server_retry) {
                tokio::time::sleep(delay).await;
            }
            attempt += 1;

            debug!("Resuming POST SSE stream from event {last_event_id}");
            let mut headers = self.session.headers(Some("text/event-stream")).await;
            if let Ok(event_value) = header::HeaderValue::from_str(&last_event_id) {
                headers.insert("Last-Event-ID", event_value);
            }
            let request = self
                .http_client
                .get(self.get_endpoint_url())
                .headers(headers)
                .send();
            match tokio::time::timeout(self.config.timeout, request).await {
                Ok(Ok(resumed)) if resumed.status() == reqwest::StatusCode::NOT_FOUND => {
                    return Err(self.expire_session().await);
                }
                Ok(Ok(resumed)) if resumed.status().is_success() => response = Some(resumed),
                Ok(Ok(resumed)) => warn!("Resuming POST SSE stream failed: {}", resumed.status()),
                Ok(Err(e)) => warn!("Resuming POST SSE stream failed: {}", e),
                Err(_) => warn!(
                    "Resuming POST SSE stream timed out after {:?}",
                    self.config.timeout
                ),
            }
        }
    }

    /// Read one connection's worth of a POST's SSE stream.
    ///
    /// Tracks the stream's cursor and the server's `retry` as it goes, so the
    /// caller can resume from exactly here.
    async fn drain_post_stream(
        &self,
        response: reqwest::Response,
        expected_id: Option<&serde_json::Value>,
        cursor: &mut Option<String>,
        server_retry: &mut Option<Duration>,
    ) -> TransportResult<PostStreamEnd> {
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        // Same buffer cap as the GET SSE loop — a buggy or malicious server that
        // streams without ever closing an event must not OOM the client.
        let buffer_cap = self
            .config
            .limits
            .enforce_on_streams
            .then_some(self.config.limits.max_response_size)
            .flatten();

        loop {
            let chunk =
                match tokio::time::timeout(self.config.sse_read_timeout, stream.next()).await {
                    Ok(Some(Ok(chunk))) => chunk,
                    Ok(Some(Err(e))) => {
                        debug!("POST SSE stream interrupted: {}", e);
                        return Ok(PostStreamEnd::Interrupted);
                    }
                    Ok(None) => return Ok(PostStreamEnd::Interrupted),
                    Err(_) => {
                        warn!(
                            "POST SSE read idle for {:?}; resuming the stream",
                            self.config.sse_read_timeout
                        );
                        return Ok(PostStreamEnd::Interrupted);
                    }
                };

            // See the matching comment in the GET SSE loop above: normalize
            // `\r\n`/lone `\r` to `\n` per chunk so the event-boundary search
            // below works against servers using either line-ending convention
            // (confirmed necessary live against a real server that emits `\r\n`).
            buffer.push_str(&normalize_sse_line_endings(&String::from_utf8_lossy(
                &chunk,
            )));

            while let Some(pos) = buffer.find("\n\n") {
                let event = parse_sse_event(&buffer[..pos]);
                buffer.drain(..pos + 2);

                advance_cursor(cursor, &event);
                if event.retry.is_some() {
                    *server_retry = event.retry;
                }
                match Self::process_post_sse_event(&event, &self.response_sender, expected_id).await
                {
                    Ok(true) => {
                        // An `initialize` answered over SSE negotiates just as
                        // one answered with JSON does.
                        if let Some(data) = event.data.as_deref() {
                            self.session
                                .capture_negotiated_version(data.as_bytes())
                                .await;
                        }
                        return Ok(PostStreamEnd::Answered);
                    }
                    Ok(false) => {}
                    Err(e) => warn!("Failed to process POST SSE event: {}", e),
                }
            }

            if let Some(cap) = buffer_cap
                && buffer.len() > cap
            {
                return Err(TransportError::ResponseTooLarge {
                    size: buffer.len(),
                    max: cap,
                });
            }
        }
    }

    /// Await the next inbound message.
    ///
    /// Unlike [`Transport::receive`] — which is non-blocking by contract and
    /// returns `None` immediately when no message is queued — this inherent
    /// method awaits on both the response and SSE channels and returns when
    /// one produces a message. This is the ergonomic choice for client code
    /// that wants a blocking `recv` without building a select loop around
    /// `receive().await`.
    pub async fn recv_async(&self) -> TransportResult<TransportMessage> {
        let mut response_receiver = self.response_receiver.lock().await;
        let mut sse_receiver = self.sse_receiver.lock().await;
        let message = tokio::select! {
            biased;
            // Prefer the response queue so synchronous POST replies land before
            // server-push SSE messages when both are ready simultaneously.
            msg = response_receiver.recv() => msg.ok_or_else(|| {
                TransportError::ConnectionLost("Response channel disconnected".to_string())
            })?,
            msg = sse_receiver.recv() => msg.ok_or_else(|| {
                TransportError::ConnectionLost("SSE channel disconnected".to_string())
            })?,
        };
        self.record_message_received(message.payload.len());
        Ok(message)
    }
}

impl Transport for StreamableHttpClientTransport {
    fn send(
        &self,
        message: TransportMessage,
    ) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
        Box::pin(async move {
            debug!("Sending message via HTTP POST");

            // Validate request size against configured limits (v2.2.0+)
            validate_request_size(message.payload.len(), &self.config.limits)?;

            // Get message endpoint (discovered or default)
            let url = self.get_message_endpoint_url().await;

            // Build headers with proper Accept negotiation
            let headers = self
                .session
                .headers(Some("application/json, text/event-stream"))
                .await;
            let had_session = headers.contains_key("mcp-session-id");

            // `timeout` covers the request up to its headers and, for a JSON
            // answer, its body. An SSE answer is read with a per-chunk idle
            // bound instead; see `read_post_stream`.
            let deadline = tokio::time::Instant::now() + self.config.timeout;
            let timed_out = || TransportError::RequestTimeout {
                operation: "HTTP POST".to_string(),
                timeout: self.config.timeout,
            };

            // Send POST request
            let request = self
                .http_client
                .post(&url)
                .headers(headers)
                .header(header::CONTENT_TYPE, "application/json")
                .body(message.payload.to_vec())
                .send();
            let response = tokio::time::timeout_at(deadline, request)
                .await
                .map_err(|_| timed_out())?
                .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;

            // A 404 on a request that carried `Mcp-Session-Id` means the
            // server no longer knows that session — it restarted, or the
            // session expired.
            if response.status() == reqwest::StatusCode::NOT_FOUND && had_session {
                return Err(self.expire_session().await);
            }

            if !response.status().is_success() {
                return Err(TransportError::ConnectionFailed(format!(
                    "POST failed: {}",
                    response.status()
                )));
            }

            // Update session ID if provided. The standalone stream starts once
            // the body has been read, so that it opens with the version this
            // response negotiated rather than the configured default.
            let assigned_session = response
                .headers()
                .get("Mcp-Session-Id")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            if let Some(session_id) = &assigned_session {
                *self.session.session_id.write().await = Some(session_id.clone());
            }

            // Check response content type and handle accordingly
            let content_type = response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();

            if response.status() == reqwest::StatusCode::ACCEPTED {
                // MCP 2025-11-25: HTTP 202 Accepted means notification/response was accepted (no body)
                debug!("Received HTTP 202 Accepted (no response body expected)");
            } else if content_type.contains("application/json") {
                // MCP 2025-11-25: Server returned immediate JSON response
                debug!("Received JSON response from POST");

                let response_bytes = tokio::time::timeout_at(deadline, response.bytes())
                    .await
                    .map_err(|_| timed_out())?
                    .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;

                // Validate response size against configured limits (v2.2.0+)
                validate_response_size(response_bytes.len(), &self.config.limits)?;

                self.session
                    .capture_negotiated_version(&response_bytes)
                    .await;

                let response_message = TransportMessage::new(
                    MessageId::from("http-response".to_string()),
                    response_bytes,
                );

                // Queue the response for the next receive() call
                self.response_sender
                    .send(response_message)
                    .await
                    .map_err(|e| TransportError::ConnectionLost(e.to_string()))?;

                debug!("JSON response queued successfully");
            } else if content_type.contains("text/event-stream") {
                // MCP 2025-11-25: Server returned SSE stream response from POST.
                //
                // Per the Streamable HTTP transport, a server MAY keep this stream open *after*
                // sending the JSON-RPC response correlated to our request (e.g. to send further
                // related messages later) — it is not required to close it. So reading must
                // stop as soon as it has queued that correlated response, not wait for the
                // stream to end; otherwise a compliant server that keeps the connection open
                // hangs this call until an unrelated operation-level timeout papers over it.
                debug!("Received SSE stream response from POST, processing events");

                // The outgoing request's own "id", so we know which SSE event is *the* response
                // versus some other message (a notification, say) the server sends first over
                // the same stream. `None` when the outgoing payload isn't a single object with an
                // "id" (e.g. a notification) — in that case any response-shaped event ends the loop.
                let expected_id: Option<serde_json::Value> =
                    serde_json::from_slice::<serde_json::Value>(&message.payload)
                        .ok()
                        .and_then(|v| v.get("id").cloned());

                // Processed inline (not spawned) to ensure proper ordering
                self.read_post_stream(response, expected_id.as_ref())
                    .await?;
                debug!("POST SSE stream processing completed");
            }

            if assigned_session.is_some() {
                self.start_sse_connection().await?;
            }

            self.record_message_sent(message.payload.len());

            debug!("Message sent successfully");
            Ok(())
        })
    }

    /// Non-blocking receive.
    ///
    /// Returns `Ok(None)` immediately when no message is queued. This is the
    /// `Transport` trait contract (polled from a select loop); it does **not**
    /// wait for the next message. Use [`Self::recv_async`] when you want to
    /// await the next message.
    fn receive(
        &self,
    ) -> Pin<Box<dyn Future<Output = TransportResult<Option<TransportMessage>>> + Send + '_>> {
        Box::pin(async move {
            // CRITICAL: Check response queue FIRST (for immediate JSON responses from POST)
            // This ensures request-response pattern works correctly per MCP 2025-11-25
            {
                let mut response_receiver = self.response_receiver.lock().await;
                match response_receiver.try_recv() {
                    Ok(message) => {
                        debug!("Received queued JSON response");
                        self.record_message_received(message.payload.len());
                        return Ok(Some(message));
                    }
                    Err(mpsc::error::TryRecvError::Empty) => {
                        // No queued responses, continue to check SSE channel
                    }
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        return Err(TransportError::ConnectionLost(
                            "Response channel disconnected".to_string(),
                        ));
                    }
                }
            }

            // Check SSE channel for server-initiated messages
            let mut sse_receiver = self.sse_receiver.lock().await;
            match sse_receiver.try_recv() {
                Ok(message) => {
                    debug!("Received SSE message");
                    self.record_message_received(message.payload.len());
                    Ok(Some(message))
                }
                Err(mpsc::error::TryRecvError::Empty) => Ok(None),
                Err(mpsc::error::TryRecvError::Disconnected) => Err(
                    TransportError::ConnectionLost("SSE channel disconnected".to_string()),
                ),
            }
        })
    }

    fn capabilities(&self) -> &TransportCapabilities {
        &self.capabilities
    }

    fn state(&self) -> Pin<Box<dyn Future<Output = TransportState> + Send + '_>> {
        Box::pin(async move { self.state.read().await.clone() })
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Http
    }

    fn metrics(&self) -> Pin<Box<dyn Future<Output = TransportMetrics> + Send + '_>> {
        Box::pin(async move { self.metrics.snapshot() })
    }

    fn connect(&self) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
        Box::pin(async move {
            info!("Connecting to {}", self.get_endpoint_url());

            *self.state.write().await = TransportState::Connecting;

            // Start SSE connection task
            self.start_sse_connection().await?;

            *self.state.write().await = TransportState::Connected;

            info!("Connected successfully");
            Ok(())
        })
    }

    fn disconnect(&self) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
        Box::pin(async move {
            info!("Disconnecting");

            *self.state.write().await = TransportState::Disconnecting;

            // Cancel SSE task
            if let Some(handle) = self.sse_task_handle.lock().await.take() {
                handle.abort();
            }

            // Send DELETE to terminate session. It carries what every other
            // request does — credentials above all, or an authenticated server
            // refuses to end the session.
            if self.session.session_id.read().await.is_some() {
                let headers = self.session.headers(None).await;
                let _ = self
                    .http_client
                    .delete(self.get_endpoint_url())
                    .headers(headers)
                    .timeout(self.config.timeout)
                    .send()
                    .await;
            }
            self.session.reset().await;

            *self.state.write().await = TransportState::Disconnected;

            info!("Disconnected");
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    /// Threshold used by the rule tests; the production default is
    /// [`DEFAULT_SSE_HEALTHY_STREAM_THRESHOLD`].
    const T: Duration = DEFAULT_SSE_HEALTHY_STREAM_THRESHOLD;

    use super::*;

    #[test]
    fn test_normalize_sse_line_endings_converts_crlf() {
        let input = "event: message\r\ndata: {\"a\":1}\r\n\r\n";
        assert_eq!(
            normalize_sse_line_endings(input),
            "event: message\ndata: {\"a\":1}\n\n"
        );
    }

    #[test]
    fn test_normalize_sse_line_endings_converts_lone_cr() {
        let input = "event: message\rdata: {\"a\":1}\r\r";
        assert_eq!(
            normalize_sse_line_endings(input),
            "event: message\ndata: {\"a\":1}\n\n"
        );
    }

    #[test]
    fn test_normalize_sse_line_endings_leaves_bare_lf_untouched() {
        let input = "event: message\ndata: {\"a\":1}\n\n";
        assert_eq!(normalize_sse_line_endings(input), input);
    }

    #[test]
    fn test_retry_policy_fixed() {
        let policy = RetryPolicy::Fixed {
            interval: Duration::from_secs(5),
            max_attempts: Some(3),
        };

        assert_eq!(policy.delay(0), Some(Duration::from_secs(5)));
        assert_eq!(policy.delay(1), Some(Duration::from_secs(5)));
        assert_eq!(policy.delay(2), Some(Duration::from_secs(5)));
        assert_eq!(policy.delay(3), None);
    }

    /// Drives the **real** reconnect loop against a server that accepts the GET and immediately
    /// closes the stream — the exact shape that produced ~50 reconnects/minute in the field.
    ///
    /// The unit tests below pin the arithmetic of `next_attempt_after_stream_end`. This one exists
    /// because that is not the same claim: it proves the loop *as written* stops hammering. Run
    /// against the pre-fix code (reset `attempt` on connect instead of on uptime) it counts
    /// connections in the hundreds and fails.
    ///
    /// A raw `TcpListener` rather than a server framework, so the test adds no dependency and
    /// models "accept, send headers, hang up" precisely.
    #[tokio::test]
    async fn a_server_that_closes_the_stream_immediately_does_not_get_hammered() {
        use std::sync::Arc as StdArc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::AsyncWriteExt;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let connections = StdArc::new(AtomicUsize::new(0));

        let accepted = StdArc::clone(&connections);
        let server = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                accepted.fetch_add(1, Ordering::Relaxed);
                // Handled off the accept loop so the listener keeps up; otherwise every other
                // connection is refused and the client takes the connect-error branch instead.
                tokio::spawn(async move {
                    let _ = sock
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .await;
                    let _ = sock.flush().await;
                    // Let the client receive the complete response before hanging up. Dropping
                    // immediately makes reqwest report a *connect* error, which takes the `Err`
                    // branch that already backs off — so the success path under test never runs.
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    drop(sock);
                });
            }
        });

        let config = StreamableHttpClientConfig {
            base_url: format!("http://{addr}"),
            retry_policy: RetryPolicy::Exponential {
                base: Duration::from_millis(200),
                max_delay: Duration::from_secs(5),
                max_attempts: None, // never give up, so this measures rate and not exhaustion
            },
            ..Default::default()
        };
        let (tx, _rx) = mpsc::channel(16);
        let task = tokio::spawn(StreamableHttpClientTransport::sse_connection_task(
            format!("http://{addr}/mcp"),
            HttpClient::new(),
            Arc::new(RwLock::new(TransportState::Disconnected)),
            tx,
            SessionState::new(Arc::new(config)),
            Arc::new(RwLock::new(None)),
        ));

        tokio::time::sleep(Duration::from_secs(2)).await;
        task.abort();
        server.abort();

        let count = connections.load(Ordering::Relaxed);
        // Measured: this loop does ~3-5 reconnects in two seconds with backoff engaged, and
        // **24,118** with the pre-fix behaviour restored (reset the counter on connect rather
        // than on uptime). The bound sits far above the former and far below the latter, so it
        // discriminates without being flaky on a slow machine.
        assert!(
            count <= 50,
            "reconnects must be rate-limited by backoff; saw {count} in 2s              (pre-fix behaviour produces ~24k)"
        );
        assert!(
            count >= 1,
            "the client should still have tried to connect at least once"
        );
    }

    /// A server that holds the stream open past the threshold and then closes it is behaving
    /// normally, and must NOT accrue backoff.
    ///
    /// This is the case measured in production: streams ended at 29.9999s, every time, against a
    /// server whose idle timeout is 30s. A threshold at or above that classified every ordinary
    /// cycle as a failure — backoff accumulated against normal operation and the client was on
    /// course to hit `max_attempts` and abandon the channel altogether.
    ///
    /// Asserted through the real loop rather than the rule alone, because the rule was already
    /// correct in isolation and the bug was in the value it was given.
    #[tokio::test]
    async fn a_stream_that_lives_past_the_threshold_is_not_treated_as_a_failure() {
        use std::sync::Arc as StdArc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::AsyncWriteExt;

        // Server holds each stream open for 300ms, then closes — "long-lived" relative to the
        // 100ms threshold below, exactly as 30s is to a 10s default.
        const HOLD: Duration = Duration::from_millis(300);
        const THRESHOLD: Duration = Duration::from_millis(100);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let connections = StdArc::new(AtomicUsize::new(0));

        let accepted = StdArc::clone(&connections);
        let server = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                accepted.fetch_add(1, Ordering::Relaxed);
                tokio::spawn(async move {
                    let _ = sock
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n",
                        )
                        .await;
                    let _ = sock.flush().await;
                    tokio::time::sleep(HOLD).await;
                    drop(sock);
                });
            }
        });

        let config = StreamableHttpClientConfig {
            base_url: format!("http://{addr}"),
            sse_healthy_stream_threshold: THRESHOLD,
            retry_policy: RetryPolicy::Exponential {
                base: Duration::from_secs(5), // huge, so any backoff at all is unmistakable
                max_delay: Duration::from_secs(30),
                max_attempts: None,
            },
            ..Default::default()
        };
        let (tx, _rx) = mpsc::channel(16);
        let task = tokio::spawn(StreamableHttpClientTransport::sse_connection_task(
            format!("http://{addr}/mcp"),
            HttpClient::new(),
            Arc::new(RwLock::new(TransportState::Disconnected)),
            tx,
            SessionState::new(Arc::new(config)),
            Arc::new(RwLock::new(None)),
        ));

        tokio::time::sleep(Duration::from_millis(1500)).await;
        task.abort();
        server.abort();

        // Each cycle costs ~300ms, so ~4 fit in 1.5s. If the threshold mis-classified these as
        // failures the 5s backoff would engage and only the first connection would ever happen.
        let count = connections.load(Ordering::Relaxed);
        assert!(
            count >= 3,
            "a healthy stream that ends must reconnect promptly, not back off; saw {count} in 1.5s"
        );
    }

    /// A stream that collapses immediately must count as a failed attempt.
    ///
    /// This is the whole bug: the old code cleared the counter the moment the GET was accepted, so
    /// a server that accepted and instantly closed produced an unthrottled reconnect loop — the
    /// `if attempt > 0` guard on the sleep meant zero delay, every time, forever.
    #[test]
    fn a_stream_that_ends_immediately_counts_as_a_failed_attempt() {
        assert_eq!(next_attempt_after_stream_end(0, Duration::ZERO, T), 1);
        assert_eq!(
            next_attempt_after_stream_end(3, Duration::from_millis(50), T),
            4
        );
        // Just under the bar is still a failure — no "close enough".
        assert_eq!(
            next_attempt_after_stream_end(1, T - Duration::from_millis(1), T),
            2
        );
    }

    /// A stream that did real work clears the backoff, so a later reconnect (a deploy, say) starts
    /// from a full set of attempts rather than inheriting an old count.
    #[test]
    fn a_long_lived_stream_resets_the_backoff() {
        assert_eq!(
            next_attempt_after_stream_end(7, T, T),
            0,
            "the threshold itself must count as healthy"
        );
        assert_eq!(
            next_attempt_after_stream_end(9, Duration::from_secs(3600), T),
            0
        );
    }

    /// The counter feeds `delay()`, whose `max_attempts` eventually gives up. Incrementing must not
    /// wrap round to 0 and restart the storm it was added to stop.
    #[test]
    fn the_attempt_counter_saturates_rather_than_wrapping() {
        assert_eq!(
            next_attempt_after_stream_end(u32::MAX, Duration::ZERO, T),
            u32::MAX
        );
    }

    /// Ties the rule back to the observable it exists to fix: with the counter rising, the policy
    /// hands back real delays instead of the zero-delay spin.
    #[test]
    fn a_flapping_stream_actually_earns_a_delay() {
        let policy = RetryPolicy::Exponential {
            base: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            max_attempts: Some(10),
        };
        let mut attempt = 0u32;
        for _ in 0..3 {
            attempt = next_attempt_after_stream_end(attempt, Duration::from_millis(10), T);
        }
        assert_eq!(attempt, 3);
        assert!(
            policy.delay(attempt).unwrap() >= Duration::from_secs(3),
            "three straight collapses must buy real backoff, not another immediate retry"
        );
    }

    #[test]
    fn test_retry_policy_exponential() {
        let policy = RetryPolicy::Exponential {
            base: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            max_attempts: None,
        };

        // With jitter, verify delays are within expected bounds
        // Expected base delays: 1s, 2s, 4s, 8s, etc. with ±25% jitter
        let delay0 = policy.delay(0).unwrap();
        assert!(delay0 >= Duration::from_millis(750) && delay0 <= Duration::from_millis(1250));

        let delay1 = policy.delay(1).unwrap();
        assert!(delay1 >= Duration::from_millis(1500) && delay1 <= Duration::from_millis(2500));

        let delay2 = policy.delay(2).unwrap();
        assert!(delay2 >= Duration::from_millis(3000) && delay2 <= Duration::from_millis(5000));

        let delay3 = policy.delay(3).unwrap();
        assert!(delay3 >= Duration::from_millis(6000) && delay3 <= Duration::from_millis(10000));

        let delay10 = policy.delay(10).unwrap();
        // Should be capped at max_delay (60s) with jitter
        assert!(delay10 >= Duration::from_millis(45000) && delay10 <= Duration::from_millis(75000));
    }

    #[tokio::test]
    async fn test_client_creation() {
        let config = StreamableHttpClientConfig::default();
        let client = StreamableHttpClientTransport::new(config).expect("default config builds");

        assert_eq!(client.transport_type(), TransportType::Http);
        assert!(client.capabilities().supports_streaming);
        assert!(client.capabilities().supports_bidirectional);
    }

    #[test]
    fn a_same_origin_endpoint_event_is_resolved_against_the_mcp_endpoint() {
        let base = "http://127.0.0.1:8080/mcp";
        for (data, expected) in [
            (
                r#"{"uri":"http://127.0.0.1:8080/messages"}"#,
                "http://127.0.0.1:8080/messages",
            ),
            (
                "http://127.0.0.1:8080/messages?s=1",
                "http://127.0.0.1:8080/messages?s=1",
            ),
            ("/messages", "http://127.0.0.1:8080/messages"),
        ] {
            assert_eq!(resolve_endpoint_event(base, data).unwrap(), expected);
        }
    }

    /// The event redirects every later POST, bearer token included, so one
    /// naming another origin must be refused — it used to be followed, which
    /// handed the credentials to whoever could put an event on the stream.
    #[test]
    fn an_endpoint_event_naming_another_origin_is_refused() {
        let base = "https://mcp.example.com/mcp";
        for data in [
            "https://evil.example/steal",
            r#"{"uri":"https://evil.example/steal"}"#,
            "http://mcp.example.com/mcp",
            "https://mcp.example.com:8443/mcp",
            "//evil.example/steal",
        ] {
            assert!(
                resolve_endpoint_event(base, data).is_err(),
                "{data} must not be accepted"
            );
        }
    }

    #[test]
    fn sse_fields_are_parsed_per_the_event_stream_rules() {
        let event = parse_sse_event(
            ": comment\nevent: message\nid: s-1-4\nretry: 2500\ndata: {\"a\":\ndata:  1}\n",
        );
        assert_eq!(event.event.as_deref(), Some("message"));
        assert_eq!(event.id.as_deref(), Some("s-1-4"));
        assert_eq!(event.retry, Some(Duration::from_millis(2500)));
        // One leading space is the separator; any more belong to the value.
        assert_eq!(event.data.as_deref(), Some("{\"a\":\n 1}"));

        // A `retry` that is not all digits is ignored, as is an id with NUL.
        let event = parse_sse_event("retry: 1.5\nid: a\0b\ndata");
        assert_eq!(event.retry, None);
        assert_eq!(event.id, None);
        assert_eq!(event.data.as_deref(), Some(""));
    }

    #[test]
    fn an_empty_id_resets_the_cursor() {
        let mut cursor = Some("s-1-3".to_string());
        advance_cursor(&mut cursor, &parse_sse_event("data: x"));
        assert_eq!(cursor.as_deref(), Some("s-1-3"), "no id leaves it alone");
        advance_cursor(&mut cursor, &parse_sse_event("id:\ndata: x"));
        assert_eq!(cursor, None);
    }

    /// §Sending Messages item 6: "The client MUST respect the `retry` field".
    #[test]
    fn the_servers_retry_is_a_floor_under_backoff() {
        let second = Duration::from_secs(1);
        assert_eq!(reconnect_delay(None, None), None);
        assert_eq!(reconnect_delay(None, Some(second)), Some(second));
        assert_eq!(reconnect_delay(Some(second), None), Some(second));
        assert_eq!(
            reconnect_delay(Some(second), Some(second * 3)),
            Some(second * 3)
        );
        assert_eq!(
            reconnect_delay(Some(second * 5), Some(second)),
            Some(second * 5)
        );
    }

    #[tokio::test]
    async fn test_post_sse_whitespace_data_event_is_ignored() {
        let (tx, mut rx) = mpsc::channel(1);

        let is_response = StreamableHttpClientTransport::process_post_sse_event(
            &parse_sse_event("id: primer-1\nevent: message\ndata:    \n"),
            &tx,
            None,
        )
        .await
        .expect("whitespace POST SSE event should be ignored");

        assert!(!is_response, "an ignored/empty event is never the response");
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_post_sse_json_event_is_queued() {
        let (tx, mut rx) = mpsc::channel(1);

        let is_response = StreamableHttpClientTransport::process_post_sse_event(
            &parse_sse_event(
                "id: msg-1\nevent: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n",
            ),
            &tx,
            None,
        )
        .await
        .expect("valid POST SSE event should be queued");

        assert!(
            is_response,
            "a result-bearing message is the correlated response"
        );
        let message = rx.try_recv().expect("queued message");
        let value: serde_json::Value =
            serde_json::from_slice(&message.payload).expect("valid queued JSON");
        assert_eq!(value["jsonrpc"], "2.0");
    }

    #[tokio::test]
    async fn test_post_sse_notification_before_response_does_not_end_the_read_loop() {
        // A server MAY send other messages (e.g. a progress notification — "method", no
        // "result"/"error") over the same POST-response stream before the actual correlated
        // response. The caller must keep reading past it, not treat it as the final response.
        let (tx, mut rx) = mpsc::channel(2);
        let expected_id = serde_json::json!(1);

        let is_response = StreamableHttpClientTransport::process_post_sse_event(
            &parse_sse_event(
                "event: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\",\"params\":{}}\n",
            ),
            &tx,
            Some(&expected_id),
        )
        .await
        .expect("notification event should be queued, not erred");
        assert!(
            !is_response,
            "a notification is never the correlated response"
        );

        let is_response = StreamableHttpClientTransport::process_post_sse_event(
            &parse_sse_event(
                "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n",
            ),
            &tx,
            Some(&expected_id),
        )
        .await
        .expect("valid POST SSE event should be queued");
        assert!(
            is_response,
            "the id-matching result IS the correlated response"
        );

        assert!(
            !rx.try_recv()
                .expect("notification queued")
                .payload
                .is_empty()
        );
        assert!(rx.try_recv().is_ok(), "response also queued");
    }

    #[tokio::test]
    async fn test_post_sse_response_with_mismatched_id_is_not_the_correlated_response() {
        // A late-arriving response to a DIFFERENT request than the one we're waiting on must not
        // be mistaken for ours — only an exact id match ends the read loop.
        let (tx, mut rx) = mpsc::channel(1);
        let expected_id = serde_json::json!(2);

        let is_response = StreamableHttpClientTransport::process_post_sse_event(
            &parse_sse_event(
                "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n",
            ),
            &tx,
            Some(&expected_id),
        )
        .await
        .expect("valid POST SSE event should be queued");

        assert!(!is_response, "id 1 does not match the expected id 2");
        assert!(
            rx.try_recv().is_ok(),
            "still queued for the caller, just not the loop's exit signal"
        );
    }

    #[tokio::test]
    async fn test_post_sse_event_with_crlf_line_endings_is_parsed() {
        // Reproduces a real server's raw bytes (confirmed via a live capture): SSE fields
        // terminated with `\r\n`, event boundary `\r\n\r\n`. Simulates the buffer normalization
        // `send()` applies per chunk before handing a complete event to this function — without
        // it, `event_str.lines()` still splits `\r\n` correctly (Rust's `lines()` already strips
        // a trailing `\r`), but the buffer-level `\n\n` boundary search upstream would never fire
        // on a `\r\n\r\n`-only stream, so the event would never reach this function at all. This
        // test exercises the normalize step directly to prove the boundary is found.
        let raw = "id: 1\r\nevent: message\r\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\r\n\r\nrest";
        let normalized = normalize_sse_line_endings(raw);
        let pos = normalized
            .find("\n\n")
            .expect("normalized buffer must expose an event boundary");
        let event = parse_sse_event(&normalized[..pos]);
        assert_eq!(event.id.as_deref(), Some("1"));

        let (tx, mut rx) = mpsc::channel(1);

        let is_response = StreamableHttpClientTransport::process_post_sse_event(&event, &tx, None)
            .await
            .expect("CRLF-terminated event should parse");

        assert!(is_response);
        let message = rx.try_recv().expect("queued message");
        let value: serde_json::Value =
            serde_json::from_slice(&message.payload).expect("valid queued JSON");
        assert_eq!(value["result"], serde_json::json!({}));
    }
}
