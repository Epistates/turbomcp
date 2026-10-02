//! [`ClientTelemetry`] — the client half (feature `client`): an OpenTelemetry
//! `CLIENT` span and an `mcp.client.operation.duration` measurement for each
//! request a [`Client`](turbomcp_client::Client) sends, with the span's trace
//! context injected into the request's `params._meta` so the server's span
//! continues it.
//!
//! ```ignore
//! use std::sync::Arc;
//! use turbomcp_client::ClientBuilder;
//! use turbomcp_telemetry::ClientTelemetry;
//!
//! let client = ClientBuilder::new("agent", "1.0")
//!     .with_observer(Arc::new(ClientTelemetry::new()))
//!     .connect(transport)
//!     .await?;
//! ```
//!
//! Spans and measurements follow the same MCP semantic conventions as the
//! server half: `{mcp.method.name} {target}` span names, `mcp.method.name`,
//! `mcp.protocol.version`, `jsonrpc.request.id`, the call's tool/prompt/URI,
//! and `error.type` / `rpc.response.status_code` on failures. A request the
//! caller abandons (dropped, cancelled) records `error.type = cancelled`.

use std::sync::Arc;
use std::time::Instant;

use opentelemetry::metrics::Histogram;
use opentelemetry::{KeyValue, global};
use serde_json::{Map, Value};
use tracing::field::Empty;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use turbomcp_client::{ClientError, OutboundRequest, RequestObserver, RequestScope};
use turbomcp_core::RequestId;

use crate::propagation;
use crate::semconv::{self, Outcome, Target};

/// Bucket boundaries, in seconds, as the MCP conventions specify.
const DURATION_BOUNDARIES: [f64; 14] = [
    0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0,
];

/// Client-side tracing and metrics, registered with
/// [`ClientBuilder::with_observer`](turbomcp_client::ClientBuilder::with_observer).
#[derive(Clone)]
pub struct ClientTelemetry {
    duration: Histogram<f64>,
    extra_methods: Arc<[String]>,
}

impl core::fmt::Debug for ClientTelemetry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ClientTelemetry").finish_non_exhaustive()
    }
}

impl Default for ClientTelemetry {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientTelemetry {
    /// Build it, reading the histogram from the global meter provider.
    #[must_use]
    pub fn new() -> Self {
        let duration = global::meter("turbomcp")
            .f64_histogram("mcp.client.operation.duration")
            .with_description(
                "Duration of an MCP request as observed by the sender, from sending it until \
                 its response is received.",
            )
            .with_unit("s")
            .with_boundaries(DURATION_BOUNDARIES.to_vec())
            .build();
        Self {
            duration,
            extra_methods: Arc::from([]),
        }
    }

    /// Name these methods in `mcp.method.name` too (an extension's, say).
    #[must_use]
    pub fn with_methods<I, M>(mut self, methods: I) -> Self
    where
        I: IntoIterator<Item = M>,
        M: Into<String>,
    {
        let mut all: Vec<String> = self.extra_methods.to_vec();
        all.extend(methods.into_iter().map(Into::into));
        self.extra_methods = all.into();
        self
    }
}

impl RequestObserver for ClientTelemetry {
    fn start(&self, request: &OutboundRequest<'_>) -> Box<dyn RequestScope> {
        let method = semconv::method_label(request.method, &self.extra_methods);
        let target = Target::from_parts(request.method, request.params);
        let name = match target.as_ref().and_then(Target::span_suffix) {
            Some(suffix) => format!("{method} {suffix}"),
            None => method.to_string(),
        };
        let span = tracing::info_span!(
            "mcp.client",
            otel.name = name.as_str(),
            otel.kind = "client",
            otel.status_code = Empty,
            otel.status_description = Empty,
            "mcp.method.name" = method.as_ref(),
            "mcp.protocol.version" = Empty,
            "mcp.resource.uri" = Empty,
            "jsonrpc.request.id" = request_id(request.id).as_str(),
            "gen_ai.tool.name" = Empty,
            "gen_ai.prompt.name" = Empty,
            "gen_ai.operation.name" = Empty,
            "error.type" = Empty,
            "rpc.response.status_code" = Empty,
        );
        let mut base = vec![KeyValue::new(
            semconv::MCP_METHOD_NAME,
            method.clone().into_owned(),
        )];
        if let Some(version) = request.protocol_version {
            let version = semconv::version_label(version);
            span.record(semconv::MCP_PROTOCOL_VERSION, version);
            base.push(KeyValue::new(semconv::MCP_PROTOCOL_VERSION, version));
        }
        match &target {
            Some(Target::Tool(name)) => {
                span.record(semconv::GEN_AI_TOOL_NAME, name.as_str());
                span.record(semconv::GEN_AI_OPERATION_NAME, "execute_tool");
            }
            Some(Target::Prompt(name)) => {
                span.record(semconv::GEN_AI_PROMPT_NAME, name.as_str());
            }
            Some(Target::Resource(uri)) => {
                span.record(semconv::MCP_RESOURCE_URI, uri.as_str());
            }
            None => {}
        }
        // "Instrumentations SHOULD propagate context ... by injecting it into
        // the MCP request `params._meta`": this span's, so the server's span
        // is its child.
        let mut meta = Map::new();
        propagation::inject(&span.context(), &mut meta);
        Box::new(ClientScope {
            span,
            meta,
            base,
            target,
            duration: self.duration.clone(),
            start: Instant::now(),
            finished: false,
        })
    }
}

fn request_id(id: &RequestId) -> String {
    match id {
        RequestId::Number(n) => n.to_string(),
        RequestId::String(s) => s.clone(),
    }
}

struct ClientScope {
    span: tracing::Span,
    meta: Map<String, Value>,
    base: Vec<KeyValue>,
    target: Option<Target>,
    duration: Histogram<f64>,
    start: Instant,
    finished: bool,
}

impl ClientScope {
    fn record(&mut self, outcome: &Outcome) {
        self.finished = true;
        if let Some(code) = outcome.status_code {
            self.span
                .record(semconv::RPC_RESPONSE_STATUS_CODE, code.to_string());
        }
        if let Some(error_type) = &outcome.error_type {
            self.span.record(semconv::ERROR_TYPE, error_type.as_ref());
            self.span.record("otel.status_code", "ERROR");
            if let Some(message) = &outcome.message {
                self.span
                    .record("otel.status_description", message.as_str());
            }
        }
        let mut labels = std::mem::take(&mut self.base);
        match &self.target {
            Some(Target::Tool(name)) => {
                labels.push(KeyValue::new(
                    semconv::GEN_AI_OPERATION_NAME,
                    "execute_tool",
                ));
                if !outcome.unknown_target {
                    labels.push(KeyValue::new(semconv::GEN_AI_TOOL_NAME, name.clone()));
                }
            }
            Some(Target::Prompt(name)) if !outcome.unknown_target => {
                labels.push(KeyValue::new(semconv::GEN_AI_PROMPT_NAME, name.clone()));
            }
            _ => {}
        }
        if let Some(error_type) = &outcome.error_type {
            labels.push(KeyValue::new(semconv::ERROR_TYPE, error_type.to_string()));
        }
        if let Some(code) = outcome.status_code {
            labels.push(KeyValue::new(
                semconv::RPC_RESPONSE_STATUS_CODE,
                code.to_string(),
            ));
        }
        self.duration
            .record(self.start.elapsed().as_secs_f64(), &labels);
    }
}

/// How a client-side failure is classified when there is no JSON-RPC error
/// to name it.
fn client_outcome(error: &ClientError) -> Outcome {
    if let Some(rpc) = error.as_rpc() {
        return Outcome::rpc_error(rpc.code, &rpc.message);
    }
    let error_type: &'static str = match error {
        ClientError::Timeout => "timeout",
        ClientError::Cancelled => "cancelled",
        ClientError::Closed | ClientError::StreamLost => "connection_lost",
        _ => "_OTHER",
    };
    let mut outcome = Outcome::other(error_type);
    outcome.message = Some(error.to_string());
    outcome
}

impl RequestScope for ClientScope {
    fn meta(&self) -> Map<String, Value> {
        self.meta.clone()
    }

    fn finish(mut self: Box<Self>, outcome: Result<&Value, &ClientError>) {
        let outcome = match outcome {
            Ok(value) => Outcome::result(value),
            Err(error) => client_outcome(error),
        };
        self.record(&outcome);
    }
}

impl Drop for ClientScope {
    fn drop(&mut self) {
        if !self.finished {
            self.record(&Outcome::cancelled());
        }
    }
}
