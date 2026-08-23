//! [`MetricsLayer`] — a [`tower::Layer`] recording OpenTelemetry metrics for
//! each MCP request: a request counter, a duration histogram, and an
//! in-flight up-down counter.
//!
//! Like [`TraceContextLayer`](crate::TraceContextLayer) it is transport-
//! agnostic (it sees `JsonRpcMessage`) and composes around a dispatcher as
//! shared RPC middleware; stack the two together for traces *and* metrics.
//! Instruments are read from the global [`opentelemetry`] meter provider, so
//! the same OTLP pipeline (`otlp` feature, or any host-installed provider)
//! exports them.
//!
//! ## Label cardinality is bounded and PII-safe
//!
//! Every metric is labeled by `mcp.method` (a fixed method name), the negotiated
//! `mcp.protocol_version`, and an `outcome` of `ok`/`error`/`cancelled` — all
//! low-cardinality and free of caller data. Identity is deliberately **not** a
//! metric label (it would be unbounded and would leak PII); identity lives on
//! spans, redacted.

use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;

use opentelemetry::metrics::{Counter, Histogram, UpDownCounter};
use opentelemetry::{KeyValue, global};
use pin_project_lite::pin_project;
use tower::{Layer, Service};
use turbomcp_core::{JsonRpcMessage, ProtocolVersion, meta};

/// The instruments, built once and shared (cheap clones — the OTel handles are
/// `Arc`-backed).
#[derive(Clone)]
struct Instruments {
    requests: Counter<u64>,
    duration: Histogram<f64>,
    in_flight: UpDownCounter<i64>,
}

impl Instruments {
    fn new() -> Self {
        let meter = global::meter("turbomcp");
        Self {
            requests: meter
                .u64_counter("mcp.server.requests")
                .with_description("Total MCP requests handled.")
                .build(),
            duration: meter
                .f64_histogram("mcp.server.request.duration")
                .with_description("MCP request handling duration.")
                .with_unit("s")
                .build(),
            in_flight: meter
                .i64_up_down_counter("mcp.server.active_requests")
                .with_description("In-flight MCP requests.")
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
}

impl MetricsLayer {
    /// Build the layer, reading instruments from the global meter provider.
    #[must_use]
    pub fn new() -> Self {
        Self {
            instruments: Arc::new(Instruments::new()),
        }
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
        }
    }
}

/// The service produced by [`MetricsLayer`].
#[derive(Clone)]
pub struct Metrics<S> {
    inner: S,
    instruments: Arc<Instruments>,
}

impl<S, E> Service<JsonRpcMessage> for Metrics<S>
where
    S: Service<JsonRpcMessage, Response = Option<JsonRpcMessage>, Error = E>,
{
    type Response = Option<JsonRpcMessage>;
    type Error = E;
    type Future = MetricsFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: JsonRpcMessage) -> Self::Future {
        // Notifications and responses are not measured as "requests"; only true
        // requests carry a method worth a metric label.
        let base_labels = match &req {
            JsonRpcMessage::Request(r) => {
                let mut labels = vec![KeyValue::new("mcp.method", r.method.clone())];
                labels.push(KeyValue::new(
                    "mcp.protocol_version",
                    protocol_version_label(&req),
                ));
                Some(labels)
            }
            _ => None,
        };
        if let Some(labels) = &base_labels {
            self.instruments.in_flight.add(1, labels);
        }
        MetricsFuture {
            inner: self.inner.call(req),
            instruments: Arc::clone(&self.instruments),
            labels: base_labels,
            start: Instant::now(),
        }
    }
}

pin_project! {
    /// Times the inner future and records the request metrics exactly once:
    /// on completion (outcome `ok`/`error`), or — if the future is dropped
    /// mid-flight (client disconnect, timeout layer) — on drop (outcome
    /// `cancelled`). Either way the in-flight counter is decremented, so the
    /// gauge cannot drift under cancellation.
    pub struct MetricsFuture<F> {
        #[pin]
        inner: F,
        instruments: Arc<Instruments>,
        // `None` for non-request messages (unmeasured), and taken once recorded.
        labels: Option<Vec<KeyValue>>,
        start: Instant,
    }

    impl<F> PinnedDrop for MetricsFuture<F> {
        fn drop(this: Pin<&mut Self>) {
            let this = this.project();
            // Labels still present ⇒ the future never completed: the request
            // was abandoned mid-flight.
            if let Some(base) = this.labels.take() {
                record(this.instruments, base, "cancelled", *this.start);
            }
        }
    }
}

/// Record one finished (or abandoned) request: count + duration with the
/// `outcome` label, and the in-flight decrement with the base labels it was
/// incremented with.
fn record(instruments: &Instruments, base: Vec<KeyValue>, outcome: &'static str, start: Instant) {
    let elapsed = start.elapsed().as_secs_f64();
    let mut labels = base;
    labels.push(KeyValue::new("outcome", outcome));
    instruments.requests.add(1, &labels);
    instruments.duration.record(elapsed, &labels);
    let in_flight_labels = &labels[..labels.len() - 1];
    instruments.in_flight.add(-1, in_flight_labels);
}

impl<F, E> Future for MetricsFuture<F>
where
    F: Future<Output = Result<Option<JsonRpcMessage>, E>>,
{
    type Output = Result<Option<JsonRpcMessage>, E>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let result = std::task::ready!(this.inner.poll(cx));

        if let Some(base) = this.labels.take() {
            let outcome = match &result {
                // A transport-level error, or a JSON-RPC error response, both
                // count as `error`; a successful/absent response is `ok`.
                Err(_) => "error",
                Ok(Some(JsonRpcMessage::Response(r))) if r.error.is_some() => "error",
                Ok(_) => "ok",
            };
            record(this.instruments, base, outcome, *this.start);
        }
        Poll::Ready(result)
    }
}

/// The negotiated protocol version label from a request's `_meta`
/// (`unknown` when absent — e.g. `initialize`, whose version is in the body).
fn protocol_version_label(req: &JsonRpcMessage) -> &'static str {
    let JsonRpcMessage::Request(r) = req else {
        return "unknown";
    };
    match turbomcp_protocol_version(r.params.as_ref()) {
        Some(ProtocolVersion::V2025_11_25) => "2025-11-25",
        Some(ProtocolVersion::V2026_07_28) => "2026-07-28",
        Some(_) => "other",
        None => "unknown",
    }
}

/// Read `_meta.io.modelcontextprotocol/protocolVersion` without depending on
/// `turbomcp-protocol` (telemetry sits below it): the key is stable core meta.
fn turbomcp_protocol_version(params: Option<&serde_json::Value>) -> Option<ProtocolVersion> {
    let version = params?
        .get("_meta")?
        .get(meta::keys::PROTOCOL_VERSION)?
        .as_str()?;
    Some(ProtocolVersion::from_wire(version))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;
    use std::task::Poll;

    use serde_json::json;
    use tower::ServiceExt;
    use turbomcp_core::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};

    #[derive(Clone)]
    struct Inner {
        fail: bool,
    }

    impl Service<JsonRpcMessage> for Inner {
        type Response = Option<JsonRpcMessage>;
        type Error = Infallible;
        type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

        fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: JsonRpcMessage) -> Self::Future {
            let reply = match req {
                JsonRpcMessage::Request(r) if self.fail => Some(
                    JsonRpcResponse::error(
                        r.id,
                        JsonRpcError {
                            code: -32000,
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
        let resp = ok.oneshot(req).await.unwrap();
        assert!(matches!(resp, Some(JsonRpcMessage::Response(_))));

        let err = MetricsLayer::new().layer(Inner { fail: true });
        let req: JsonRpcMessage = JsonRpcRequest::new(2, "tools/call", None).into();
        let resp = err.oneshot(req).await.unwrap();
        let Some(JsonRpcMessage::Response(r)) = resp else {
            panic!("expected response")
        };
        assert!(r.error.is_some());
    }

    #[test]
    fn version_label_reads_meta() {
        let draft: JsonRpcMessage = JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" } })),
        )
        .into();
        assert_eq!(protocol_version_label(&draft), "2026-07-28");

        let bare: JsonRpcMessage = JsonRpcRequest::new(1, "ping", None).into();
        assert_eq!(protocol_version_label(&bare), "unknown");
    }

    /// An inner service whose future never resolves — the stand-in for a
    /// handler abandoned mid-flight (client disconnect, timeout layer).
    #[derive(Clone)]
    struct Never;

    impl Service<JsonRpcMessage> for Never {
        type Response = Option<JsonRpcMessage>;
        type Error = Infallible;
        type Future = std::future::Pending<Result<Self::Response, Self::Error>>;

        fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _: JsonRpcMessage) -> Self::Future {
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
        let mut svc = MetricsLayer::new().layer(Never);
        let req: JsonRpcMessage = JsonRpcRequest::new(1, "drop-probe", None).into();
        let fut = svc.call(req); // in-flight +1
        drop(fut); // abandoned before completion

        provider.force_flush().unwrap();
        let finished = exporter.get_finished_metrics().unwrap();

        assert_eq!(
            sum_with(
                &finished,
                "mcp.server.requests",
                &[("mcp.method", "drop-probe"), ("outcome", "cancelled")],
            ),
            1,
            "an abandoned request counts once, as cancelled"
        );
        assert_eq!(
            sum_with(
                &finished,
                "mcp.server.active_requests",
                &[("mcp.method", "drop-probe")],
            ),
            0,
            "the in-flight gauge returns to zero — no drift under cancellation"
        );
    }

    #[test]
    fn composes_as_mcp_service() {
        fn assert_mcp_service<S: turbomcp_service::McpService>(_: &S) {}
        #[derive(Clone)]
        struct Dispatcher;
        impl Service<JsonRpcMessage> for Dispatcher {
            type Response = Option<JsonRpcMessage>;
            type Error = turbomcp_service::ProtocolError;
            type Future = std::future::Ready<Result<Self::Response, Self::Error>>;
            fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
                Poll::Ready(Ok(()))
            }
            fn call(&mut self, _: JsonRpcMessage) -> Self::Future {
                std::future::ready(Ok(None))
            }
        }
        assert_mcp_service(&MetricsLayer::new().layer(Dispatcher));
    }
}
