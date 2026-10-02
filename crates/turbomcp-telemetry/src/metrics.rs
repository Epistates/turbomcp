//! [`MetricsLayer`] — a [`tower::Layer`] recording the MCP semantic
//! conventions' `mcp.server.operation.duration` histogram for each request,
//! plus an in-flight counter.
//!
//! Like [`TraceContextLayer`](crate::TraceContextLayer) it is transport-
//! agnostic (it sees `JsonRpcMessage`) and composes around a dispatcher as
//! shared RPC middleware; stack the two together for traces *and* metrics.
//! Instruments are read from the global [`opentelemetry`] meter provider, so
//! the same OTLP pipeline (`otlp` feature, or any host-installed provider)
//! exports them.
//!
//! ## Attributes, and why their cardinality is bounded
//!
//! Each measurement carries `mcp.method.name`, `mcp.protocol.version`,
//! `gen_ai.operation.name = execute_tool` on tool calls, the call's
//! `gen_ai.tool.name` / `gen_ai.prompt.name`, and, when they apply,
//! `error.type` and `rpc.response.status_code`. The method comes off the wire,
//! so it is one of the spec's names (or one registered with
//! [`MetricsLayer::with_methods`]) or `_OTHER`; the version is a supported
//! revision or `other`. A tool or prompt name is recorded only when the server
//! knew it (the call didn't fail with `-32601`/`-32602`), so it is bounded by
//! the server's own catalogue rather than by what callers invent. Resource
//! URIs (opt-in in the conventions) and identity are never labels: unbounded,
//! and identity is PII.

use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;

use opentelemetry::metrics::{Histogram, UpDownCounter};
use opentelemetry::{KeyValue, global};
use pin_project_lite::pin_project;
use tower::{Layer, Service};
use turbomcp_core::{JsonRpcMessage, McpRequest};

use crate::semconv::{self, Outcome, Target};

/// Bucket boundaries for the duration histogram, in seconds: the ones the
/// MCP conventions specify. The SDK's defaults are millisecond-scale.
const DURATION_BOUNDARIES: [f64; 14] = [
    0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0,
];

/// The instruments, built once and shared (cheap clones — the OTel handles are
/// `Arc`-backed).
#[derive(Clone)]
struct Instruments {
    duration: Histogram<f64>,
    in_flight: UpDownCounter<i64>,
}

impl Instruments {
    fn new() -> Self {
        let meter = global::meter("turbomcp");
        Self {
            duration: meter
                .f64_histogram("mcp.server.operation.duration")
                .with_description(
                    "Duration of an MCP request or notification as observed by the receiver, \
                     from receipt until the result or acknowledgement is sent.",
                )
                .with_unit("s")
                .with_boundaries(DURATION_BOUNDARIES.to_vec())
                .build(),
            // The conventions define no in-flight instrument, hence the vendor
            // prefix.
            in_flight: meter
                .i64_up_down_counter("turbomcp.server.active_operations")
                .with_description("MCP requests in flight.")
                .build(),
        }
    }
}

/// A [`tower::Layer`] recording per-request OpenTelemetry metrics. Compose it
/// around a dispatcher like any shared RPC middleware (typically alongside
/// [`TraceContextLayer`](crate::TraceContextLayer)).
#[derive(Clone)]
pub struct MetricsLayer {
    instruments: Arc<Instruments>,
    extra_methods: Arc<[String]>,
}

impl MetricsLayer {
    /// Build the layer, reading instruments from the global meter provider.
    #[must_use]
    pub fn new() -> Self {
        Self {
            instruments: Arc::new(Instruments::new()),
            extra_methods: Arc::from([]),
        }
    }

    /// Label these methods by name too, alongside the spec's own: an
    /// extension's methods, say. Anything outside the set is `_OTHER`.
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

impl Default for MetricsLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for MetricsLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MetricsLayer")
    }
}

impl<S> Layer<S> for MetricsLayer {
    type Service = Metrics<S>;

    fn layer(&self, inner: S) -> Self::Service {
        Metrics {
            inner,
            instruments: Arc::clone(&self.instruments),
            extra_methods: Arc::clone(&self.extra_methods),
        }
    }
}

/// The service produced by [`MetricsLayer`].
#[derive(Clone)]
pub struct Metrics<S> {
    inner: S,
    instruments: Arc<Instruments>,
    extra_methods: Arc<[String]>,
}

impl<S> core::fmt::Debug for Metrics<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Metrics").finish_non_exhaustive()
    }
}

/// What a measurement is labelled with before the outcome is known.
struct Pending {
    /// `mcp.method.name` and `mcp.protocol.version`: also the in-flight
    /// counter's labels.
    base: Vec<KeyValue>,
    target: Option<Target>,
}

impl<S, E> Service<McpRequest> for Metrics<S>
where
    S: Service<McpRequest, Response = Option<JsonRpcMessage>, Error = E>,
{
    type Response = Option<JsonRpcMessage>;
    type Error = E;
    type Future = MetricsFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: McpRequest) -> Self::Future {
        // Only requests are measured: they are the operations with a result.
        let pending = match &req.message {
            JsonRpcMessage::Request(r) => {
                let mut base = vec![KeyValue::new(
                    semconv::MCP_METHOD_NAME,
                    semconv::method_label(&r.method, &self.extra_methods).into_owned(),
                )];
                if let Some(version) = semconv::protocol_version(&req.message) {
                    base.push(KeyValue::new(semconv::MCP_PROTOCOL_VERSION, version));
                }
                if let Some(network) = req.extensions.get::<turbomcp_service::NetworkFacts>() {
                    base.extend(semconv::network_labels(network));
                }
                Some(Pending {
                    base,
                    target: Target::of(&req.message),
                })
            }
            _ => None,
        };
        if let Some(pending) = &pending {
            self.instruments.in_flight.add(1, &pending.base);
        }
        MetricsFuture {
            inner: self.inner.call(req),
            instruments: Arc::clone(&self.instruments),
            pending,
            start: Instant::now(),
        }
    }
}

pin_project! {
    /// Times the inner future and records the measurement exactly once: on
    /// completion, or, if the future is dropped mid-flight (client
    /// disconnect, timeout layer), on drop as `error.type = cancelled`.
    /// Either way the in-flight counter is decremented, so it cannot drift
    /// under cancellation.
    pub struct MetricsFuture<F> {
        #[pin]
        inner: F,
        instruments: Arc<Instruments>,
        // `None` for non-request messages (unmeasured), and taken once recorded.
        pending: Option<Pending>,
        start: Instant,
    }

    impl<F> PinnedDrop for MetricsFuture<F> {
        fn drop(this: Pin<&mut Self>) {
            let this = this.project();
            if let Some(pending) = this.pending.take() {
                record(this.instruments, pending, &Outcome::cancelled(), *this.start);
            }
        }
    }
}

/// Record one finished (or abandoned) request.
fn record(instruments: &Instruments, pending: Pending, outcome: &Outcome, start: Instant) {
    let elapsed = start.elapsed().as_secs_f64();
    instruments.in_flight.add(-1, &pending.base);
    let mut labels = pending.base;
    match &pending.target {
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
    instruments.duration.record(elapsed, &labels);
}

impl<F, E> Future for MetricsFuture<F>
where
    F: Future<Output = Result<Option<JsonRpcMessage>, E>>,
{
    type Output = Result<Option<JsonRpcMessage>, E>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let result = std::task::ready!(this.inner.poll(cx));
        if let Some(pending) = this.pending.take() {
            record(
                this.instruments,
                pending,
                &Outcome::of(&result),
                *this.start,
            );
        }
        Poll::Ready(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;
    use std::task::Poll;

    use serde_json::json;
    use tower::ServiceExt;
    use turbomcp_core::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, ProtocolVersion};

    #[derive(Clone)]
    struct Inner {
        fail: bool,
    }

    impl Service<McpRequest> for Inner {
        type Response = Option<JsonRpcMessage>;
        type Error = Infallible;
        type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

        fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, request: McpRequest) -> Self::Future {
            let req = request.message;
            let reply = match req {
                JsonRpcMessage::Request(r) if self.fail => Some(
                    JsonRpcResponse::error(
                        r.id,
                        JsonRpcError {
                            code: turbomcp_core::codes::SERVER_ERROR,
                            message: "boom".into(),
                            data: None,
                        },
                    )
                    .into(),
                ),
                JsonRpcMessage::Request(r) => {
                    Some(JsonRpcResponse::success(r.id, json!({})).into())
                }
                _ => None,
            };
            std::future::ready(Ok(reply))
        }
    }

    #[tokio::test]
    async fn records_ok_and_error_without_panicking() {
        // Without a global meter provider installed, the instruments are no-ops;
        // the layer must still pass requests through cleanly (metrics are
        // best-effort observability, never load-bearing).
        let ok = MetricsLayer::new().layer(Inner { fail: false });
        let req: JsonRpcMessage = JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" } })),
        )
        .into();
        let resp = ok.oneshot(req.into()).await.unwrap();
        assert!(matches!(resp, Some(JsonRpcMessage::Response(_))));

        let err = MetricsLayer::new().layer(Inner { fail: true });
        let req: JsonRpcMessage = JsonRpcRequest::new(2, "tools/call", None).into();
        let resp = err.oneshot(req.into()).await.unwrap();
        let Some(JsonRpcMessage::Response(r)) = resp else {
            panic!("expected response")
        };
        assert!(r.error.is_some());
    }

    /// A method off the wire is a label only if it is one we know, or the
    /// series count is whatever a caller wants it to be.
    #[test]
    fn unknown_methods_share_one_label() {
        use crate::semconv::method_label;
        assert_eq!(method_label("tools/call", &[]), "tools/call");
        assert_eq!(method_label("x/made-up-1", &[]), "_OTHER");
        let extra = vec!["acme/export".to_owned()];
        assert_eq!(method_label("acme/export", &extra), "acme/export");
    }

    #[test]
    fn every_supported_revision_is_labelled_by_name() {
        for version in ProtocolVersion::SUPPORTED {
            let req: JsonRpcMessage = JsonRpcRequest::new(
                1,
                "ping",
                Some(json!({ "_meta": { "io.modelcontextprotocol/protocolVersion": version.as_str() } })),
            )
            .into();
            assert_eq!(semconv::protocol_version(&req), Some(version.as_str()));
        }
        let odd: JsonRpcMessage = JsonRpcRequest::new(
            1,
            "ping",
            Some(json!({ "_meta": { "io.modelcontextprotocol/protocolVersion": "1999-01-01" } })),
        )
        .into();
        assert_eq!(semconv::protocol_version(&odd), Some("other"));
    }

    #[test]
    fn version_label_reads_meta() {
        let draft: JsonRpcMessage = JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" } })),
        )
        .into();
        assert_eq!(semconv::protocol_version(&draft), Some("2026-07-28"));

        let bare: JsonRpcMessage = JsonRpcRequest::new(1, "ping", None).into();
        assert_eq!(semconv::protocol_version(&bare), None);
    }

    /// An inner service whose future never resolves — the stand-in for a
    /// handler abandoned mid-flight (client disconnect, timeout layer).
    #[derive(Clone)]
    struct Never;

    impl Service<McpRequest> for Never {
        type Response = Option<JsonRpcMessage>;
        type Error = Infallible;
        type Future = std::future::Pending<Result<Self::Response, Self::Error>>;

        fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _: McpRequest) -> Self::Future {
            std::future::pending()
        }
    }

    /// Total of every `name` sum data point whose attributes include all of
    /// `want`, in the latest exported snapshot (cumulative temporality). The
    /// filter keys on this test's unique method label, so concurrent tests
    /// recording through the same global provider can't interfere.
    fn sum_with(
        finished: &[opentelemetry_sdk::metrics::data::ResourceMetrics],
        name: &str,
        want: &[(&str, &str)],
    ) -> i128 {
        use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
        let mut total: i128 = 0;
        let Some(snapshot) = finished.last() else {
            return 0;
        };
        for scope in snapshot.scope_metrics() {
            for metric in scope.metrics() {
                if metric.name() != name {
                    continue;
                }
                match metric.data() {
                    AggregatedMetrics::U64(MetricData::Sum(sum)) => {
                        for dp in sum.data_points() {
                            if attrs_match(dp.attributes(), want) {
                                total += i128::from(dp.value());
                            }
                        }
                    }
                    AggregatedMetrics::I64(MetricData::Sum(sum)) => {
                        for dp in sum.data_points() {
                            if attrs_match(dp.attributes(), want) {
                                total += i128::from(dp.value());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        total
    }

    /// Total count of every `name` histogram data point whose attributes
    /// include all of `want`.
    fn histogram_count_with(
        finished: &[opentelemetry_sdk::metrics::data::ResourceMetrics],
        name: &str,
        want: &[(&str, &str)],
    ) -> u64 {
        use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
        let Some(snapshot) = finished.last() else {
            return 0;
        };
        snapshot
            .scope_metrics()
            .flat_map(|scope| scope.metrics())
            .filter(|m| m.name() == name)
            .map(|m| match m.data() {
                AggregatedMetrics::F64(MetricData::Histogram(h)) => h
                    .data_points()
                    .filter(|dp| attrs_match(dp.attributes(), want))
                    .map(|dp| dp.count())
                    .sum(),
                _ => 0,
            })
            .sum()
    }

    fn attrs_match<'a>(attrs: impl Iterator<Item = &'a KeyValue>, want: &[(&str, &str)]) -> bool {
        let attrs: Vec<&KeyValue> = attrs.collect();
        want.iter().all(|(k, v)| {
            attrs
                .iter()
                .any(|kv| kv.key.as_str() == *k && kv.value.as_str() == *v)
        })
    }

    #[tokio::test]
    async fn dropped_mid_flight_request_is_cancelled_and_frees_the_gauge() {
        use opentelemetry_sdk::metrics::{
            InMemoryMetricExporter, PeriodicReader, SdkMeterProvider,
        };

        let exporter = InMemoryMetricExporter::default();
        let provider = SdkMeterProvider::builder()
            .with_reader(PeriodicReader::builder(exporter.clone()).build())
            .build();
        global::set_meter_provider(provider.clone());

        // Instruments bind to the (now SDK-backed) global provider at layer
        // construction.
        let mut svc = MetricsLayer::new()
            .with_methods(["drop-probe"])
            .layer(Never);
        let req: JsonRpcMessage = JsonRpcRequest::new(1, "drop-probe", None).into();
        let fut = svc.call(req.into()); // in-flight +1
        drop(fut); // abandoned before completion

        provider.force_flush().unwrap();
        let finished = exporter.get_finished_metrics().unwrap();

        assert_eq!(
            histogram_count_with(
                &finished,
                "mcp.server.operation.duration",
                &[
                    ("mcp.method.name", "drop-probe"),
                    ("error.type", "cancelled")
                ],
            ),
            1,
            "an abandoned request counts once, as cancelled"
        );
        assert_eq!(
            sum_with(
                &finished,
                "turbomcp.server.active_operations",
                &[("mcp.method.name", "drop-probe")],
            ),
            0,
            "the in-flight gauge returns to zero — no drift under cancellation"
        );

        // The duration histogram buckets in seconds, not the SDK's
        // millisecond-scale defaults.
        use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
        let snapshot = finished.last().expect("a snapshot");
        let bounds: Vec<f64> = snapshot
            .scope_metrics()
            .flat_map(|scope| scope.metrics())
            .filter(|m| m.name() == "mcp.server.operation.duration")
            .find_map(|m| match m.data() {
                AggregatedMetrics::F64(MetricData::Histogram(h)) => {
                    h.data_points().next().map(|dp| dp.bounds().collect())
                }
                _ => None,
            })
            .expect("a duration data point");
        assert_eq!(bounds, DURATION_BOUNDARIES.to_vec());
    }

    #[test]
    fn composes_as_mcp_service() {
        fn assert_mcp_service<S: turbomcp_service::McpService>(_: &S) {}
        #[derive(Clone)]
        struct Dispatcher;
        impl Service<McpRequest> for Dispatcher {
            type Response = Option<JsonRpcMessage>;
            type Error = turbomcp_service::ProtocolError;
            type Future = std::future::Ready<Result<Self::Response, Self::Error>>;
            fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
                Poll::Ready(Ok(()))
            }
            fn call(&mut self, _: McpRequest) -> Self::Future {
                std::future::ready(Ok(None))
            }
        }
        assert_mcp_service(&MetricsLayer::new().layer(Dispatcher));
    }
}
