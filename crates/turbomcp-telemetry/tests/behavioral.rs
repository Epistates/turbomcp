//! Behavioral tests for the telemetry tower service.
//!
//! Pre-3.1 the only telemetry tests asserted on the constant strings used as
//! span field names — they passed even when no spans were ever recorded.
//! These tests drive the actual `TelemetryService::call` path and assert that
//! a span is emitted with the expected fields populated by an MCP request.
//!
//! The capturing layer folds `on_record` into the span it belongs to, so a
//! field set with `Span::record` after creation shows up here exactly when a
//! real subscriber (fmt, OpenTelemetry) would see it. Before 3.5.0 the
//! completion fields were recorded without being declared on the span, and
//! `tracing` drops those silently; these tests are what would have caught it.

#![cfg(feature = "tower")]

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use tower::Service;
use tower::ServiceExt;

use tracing::Subscriber;
use tracing::field::Visit;
use tracing::span;
use tracing_subscriber::Layer;
use tracing_subscriber::Registry;
use tracing_subscriber::layer::Context;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;

use turbomcp_telemetry::tower::{TelemetryLayer, TelemetryLayerConfig};

#[derive(Clone, Default)]
struct CapturingLayer {
    spans: Arc<Mutex<Vec<RecordedSpan>>>,
}

#[derive(Debug, Clone, Default)]
struct RecordedSpan {
    name: String,
    fields: HashMap<String, String>,
}

/// Position of a span in [`CapturingLayer::spans`], stashed in the span's
/// extensions so `on_record` can find it again.
struct SpanIndex(usize);

struct FieldVisitor<'a> {
    fields: &'a mut HashMap<String, String>,
}

impl Visit for FieldVisitor<'_> {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.fields
            .insert(field.name().to_string(), value.to_string());
    }
    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.fields
            .insert(field.name().to_string(), value.to_string());
    }
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.fields
            .insert(field.name().to_string(), value.to_string());
    }
    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), value.to_string());
    }
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.fields
            .insert(field.name().to_string(), format!("{value:?}"));
    }
}

impl<S> Layer<S> for CapturingLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
        let mut fields = HashMap::new();
        attrs.record(&mut FieldVisitor {
            fields: &mut fields,
        });
        let mut spans = self.spans.lock().unwrap();
        spans.push(RecordedSpan {
            name: attrs.metadata().name().to_string(),
            fields,
        });
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(SpanIndex(spans.len() - 1));
        }
    }

    fn on_record(&self, id: &span::Id, values: &span::Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let extensions = span.extensions();
        let Some(SpanIndex(index)) = extensions.get::<SpanIndex>() else {
            return;
        };
        let mut spans = self.spans.lock().unwrap();
        values.record(&mut FieldVisitor {
            fields: &mut spans[*index].fields,
        });
    }
}

impl CapturingLayer {
    /// The single `mcp.request` span the service is expected to emit.
    fn request_span(&self) -> RecordedSpan {
        let spans = self.spans.lock().unwrap();
        let mut requests = spans.iter().filter(|s| s.name == "mcp.request");
        let span = requests
            .next()
            .unwrap_or_else(|| panic!("no `mcp.request` span; got {spans:#?}"))
            .clone();
        assert!(
            requests.next().is_none(),
            "more than one `mcp.request` span"
        );
        span
    }
}

#[derive(Clone)]
struct EchoService;

impl Service<serde_json::Value> for EchoService {
    type Response = serde_json::Value;
    type Error = Infallible;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: serde_json::Value) -> Self::Future {
        Box::pin(async move {
            let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);
            Ok(serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "ok": true }
            }))
        })
    }
}

/// Run one JSON-RPC request through a `TelemetryLayer` built from `config`
/// under a capturing subscriber, and return the layer.
async fn capture_json(
    config: TelemetryLayerConfig,
    inner: impl Service<
        serde_json::Value,
        Response = serde_json::Value,
        Error = Infallible,
        Future: Send,
    > + Clone
    + Send
    + 'static,
    req: serde_json::Value,
) -> CapturingLayer {
    let layer = CapturingLayer::default();
    let subscriber = Registry::default().with(layer.clone());
    let _guard = tracing::subscriber::set_default(subscriber);

    let svc = tower::ServiceBuilder::new()
        .layer(TelemetryLayer::new(config))
        .service(inner);
    let _ = svc.oneshot(req).await.unwrap();

    layer
}

#[tokio::test(flavor = "current_thread")]
async fn telemetry_service_records_span_for_tools_call() {
    let layer = capture_json(
        TelemetryLayerConfig::default(),
        EchoService,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/call",
            "params": { "name": "calculator" }
        }),
    )
    .await;

    let span = layer.request_span();
    assert_eq!(span.fields["mcp.method"], "tools/call");
    assert_eq!(span.fields["mcp.tool.name"], "calculator");
    assert_eq!(span.fields["mcp.request.id"], "7");
}

#[tokio::test(flavor = "current_thread")]
async fn successful_request_records_duration_and_status() {
    let layer = capture_json(
        TelemetryLayerConfig::default(),
        EchoService,
        serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
    )
    .await;

    let span = layer.request_span();
    assert!(
        span.fields.contains_key("mcp.duration_ms"),
        "mcp.duration_ms never reached the subscriber: {span:#?}"
    );
    assert_eq!(
        span.fields.get("mcp.status").map(String::as_str),
        Some("success")
    );
    assert!(!span.fields.contains_key("mcp.error.code"));
    assert!(!span.fields.contains_key("mcp.error.message"));
}

#[tokio::test(flavor = "current_thread")]
async fn json_rpc_error_response_records_status_code_and_message() {
    let failing = tower::service_fn(|req: serde_json::Value| async move {
        Ok::<_, Infallible>(serde_json::json!({
            "jsonrpc": "2.0",
            "id": req["id"],
            "error": { "code": -32602, "message": "unknown tool: nope" }
        }))
    });

    let layer = capture_json(
        TelemetryLayerConfig::default(),
        failing,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": { "name": "nope" }
        }),
    )
    .await;

    let span = layer.request_span();
    assert_eq!(
        span.fields.get("mcp.status").map(String::as_str),
        Some("error")
    );
    assert_eq!(
        span.fields.get("mcp.error.code").map(String::as_str),
        Some("-32602")
    );
    assert_eq!(
        span.fields.get("mcp.error.message").map(String::as_str),
        Some("unknown tool: nope")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn http_request_records_duration_and_status() {
    let layer = CapturingLayer::default();
    let subscriber = Registry::default().with(layer.clone());
    let _guard = tracing::subscriber::set_default(subscriber);

    let inner = tower::service_fn(|_req: http::Request<()>| async {
        Ok::<_, Infallible>(http::Response::new(()))
    });
    let svc = tower::ServiceBuilder::new()
        .layer(TelemetryLayer::new(TelemetryLayerConfig::default()))
        .service(inner);
    let req = http::Request::builder().uri("/mcp").body(()).unwrap();
    let _ = svc.oneshot(req).await.unwrap();

    let span = layer.request_span();
    assert_eq!(span.fields["mcp.transport"], "http");
    assert!(span.fields.contains_key("mcp.duration_ms"), "{span:#?}");
    assert_eq!(
        span.fields.get("mcp.status").map(String::as_str),
        Some("success")
    );
}

fn resources_read(uri: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "resources/read",
        "params": { "uri": uri }
    })
}

#[tokio::test(flavor = "current_thread")]
async fn resource_uri_is_not_recorded_by_default() {
    let layer = capture_json(
        TelemetryLayerConfig::default(),
        EchoService,
        resources_read("https://files.example/alice/report.pdf?token=s3cr3t"),
    )
    .await;

    let span = layer.request_span();
    assert!(
        !span.fields.contains_key("mcp.resource.uri"),
        "resource URI leaked into telemetry by default: {span:#?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn resource_uri_is_recorded_when_opted_in() {
    let layer = capture_json(
        TelemetryLayerConfig::default().redact_resource_uri(false),
        EchoService,
        resources_read("file:///srv/public/readme.md"),
    )
    .await;

    let span = layer.request_span();
    assert_eq!(
        span.fields.get("mcp.resource.uri").map(String::as_str),
        Some("file:///srv/public/readme.md")
    );
}

/// W3C trace-context extraction needs a tracing-opentelemetry layer to attach
/// the remote parent to, so these run only with the `opentelemetry` feature.
#[cfg(feature = "opentelemetry")]
mod propagation {
    use super::*;

    use opentelemetry::trace::{TraceContextExt, TraceId, TracerProvider as _};
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use tracing::Instrument;
    use tracing_opentelemetry::OpenTelemetrySpanExt;

    const TRACE_ID: &str = "0af7651916cd43dd8448eb211c80319c";
    const TRACEPARENT: &str = "00-0af7651916cd43dd8448eb211c80319c-00f067aa0ba902b7-01";

    /// Trace ID of whatever span is current when the inner service runs.
    type Seen = Arc<Mutex<Option<TraceId>>>;

    fn current_trace_id() -> TraceId {
        tracing::Span::current()
            .context()
            .span()
            .span_context()
            .trace_id()
    }

    fn otel_subscriber() -> impl Subscriber + Send + Sync {
        let tracer = SdkTracerProvider::builder().build().tracer("test");
        Registry::default().with(tracing_opentelemetry::layer().with_tracer(tracer))
    }

    fn recording_json_service(
        seen: Seen,
    ) -> impl Service<
        serde_json::Value,
        Response = serde_json::Value,
        Error = Infallible,
        Future: Send,
    > + Clone {
        tower::service_fn(move |_req: serde_json::Value| {
            *seen.lock().unwrap() = Some(current_trace_id());
            async { Ok::<_, Infallible>(serde_json::json!({ "jsonrpc": "2.0", "result": {} })) }
        })
    }

    #[tokio::test(flavor = "current_thread")]
    async fn json_rpc_request_joins_trace_from_meta_traceparent() {
        let _guard = tracing::subscriber::set_default(otel_subscriber());
        let seen = Seen::default();

        let svc = tower::ServiceBuilder::new()
            .layer(TelemetryLayer::new(TelemetryLayerConfig::default()))
            .service(recording_json_service(seen.clone()));
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": "x", "_meta": { "traceparent": TRACEPARENT } }
        });
        let _ = svc.oneshot(req).await.unwrap();

        assert_eq!(
            seen.lock().unwrap().unwrap(),
            TraceId::from_hex(TRACE_ID).unwrap()
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn http_request_joins_trace_from_traceparent_header() {
        let _guard = tracing::subscriber::set_default(otel_subscriber());
        let seen = Seen::default();

        let inner = {
            let seen = seen.clone();
            tower::service_fn(move |_req: http::Request<()>| {
                *seen.lock().unwrap() = Some(current_trace_id());
                async { Ok::<_, Infallible>(http::Response::new(())) }
            })
        };
        let svc = tower::ServiceBuilder::new()
            .layer(TelemetryLayer::new(TelemetryLayerConfig::default()))
            .service(inner);
        let req = http::Request::builder()
            .uri("/mcp")
            .header("traceparent", TRACEPARENT)
            .body(())
            .unwrap();
        let _ = svc.oneshot(req).await.unwrap();

        assert_eq!(
            seen.lock().unwrap().unwrap(),
            TraceId::from_hex(TRACE_ID).unwrap()
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn request_without_traceparent_keeps_its_local_parent() {
        let _guard = tracing::subscriber::set_default(otel_subscriber());
        let seen = Seen::default();

        let outer = tracing::info_span!("outer");
        let outer_trace = outer.context().span().span_context().trace_id();

        let svc = tower::ServiceBuilder::new()
            .layer(TelemetryLayer::new(TelemetryLayerConfig::default()))
            .service(recording_json_service(seen.clone()));
        let req = serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
        let _ = svc.oneshot(req).instrument(outer).await.unwrap();

        assert_eq!(seen.lock().unwrap().unwrap(), outer_trace);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn propagation_can_be_disabled() {
        let _guard = tracing::subscriber::set_default(otel_subscriber());
        let seen = Seen::default();

        let svc = tower::ServiceBuilder::new()
            .layer(TelemetryLayer::new(
                TelemetryLayerConfig::default().propagate_context(false),
            ))
            .service(recording_json_service(seen.clone()));
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
            "params": { "_meta": { "traceparent": TRACEPARENT } }
        });
        let _ = svc.oneshot(req).await.unwrap();

        assert_ne!(
            seen.lock().unwrap().unwrap(),
            TraceId::from_hex(TRACE_ID).unwrap()
        );
    }
}
