//! Behavioral tests for the telemetry tower service.
//!
//! Pre-3.1 the only telemetry tests asserted on the constant strings used as
//! span field names — they passed even when no spans were ever recorded.
//! These tests drive the actual `TelemetryService::call` path and assert that
//! a span is emitted with the expected fields populated by an MCP request.

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
    fn on_new_span(&self, attrs: &span::Attributes<'_>, _id: &span::Id, _ctx: Context<'_, S>) {
        let mut fields = HashMap::new();
        attrs.record(&mut FieldVisitor {
            fields: &mut fields,
        });
        self.spans.lock().unwrap().push(RecordedSpan {
            name: attrs.metadata().name().to_string(),
            fields,
        });
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

#[tokio::test(flavor = "current_thread")]
async fn telemetry_service_records_span_for_tools_call() {
    let layer = CapturingLayer::default();
    let spans = layer.spans.clone();
    let subscriber = Registry::default().with(layer);
    let _guard = tracing::subscriber::set_default(subscriber);

    let svc = tower::ServiceBuilder::new()
        .layer(TelemetryLayer::new(TelemetryLayerConfig::default()))
        .service(EchoService);

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "tools/call",
        "params": { "name": "calculator" }
    });

    let _ = svc.oneshot(req).await.unwrap();

    let recorded = spans.lock().unwrap().clone();
    assert!(
        !recorded.is_empty(),
        "TelemetryService did not record any spans"
    );

    let has_method = recorded
        .iter()
        .any(|s| s.fields.get("mcp.method").map(String::as_str) == Some("tools/call"));
    assert!(
        has_method,
        "no span carried mcp.method=tools/call. Spans: {recorded:#?}"
    );

    // The "request" span must be present and tagged with the MCP method. Other
    // attributes (`mcp.tool.name`, `mcp.duration_ms`, etc.) are recorded onto
    // the span via `Span::record(...)` after creation; whether they make it
    // into a particular subscriber depends on the subscriber's filter — what
    // matters here is that *some* MCP-shaped span fired with the method tag,
    // which proves the layer/service is wired and is not a no-op.
    let has_request_span = recorded.iter().any(|s| s.name == "mcp.request");
    assert!(
        has_request_span,
        "expected an `mcp.request` span; got names: {:?}",
        recorded.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
}
