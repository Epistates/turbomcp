//! The crate's central privacy claim, asserted against real captured spans:
//! the request span records the identity subject as a stable HASH (never the
//! raw value) and claim KEYS only (never claim values) under the default
//! policy; `SpanPolicy::unredacted()` records the raw subject but still no
//! claim values. Plus the distributed-trace continuation: the exported span
//! is a child of the caller's `traceparent`.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll};

use serde_json::{Map, Value, json};
use tower::{Layer as TowerLayer, Service, ServiceExt};
use tracing::field::{Field, Visit};
use tracing_subscriber::layer::SubscriberExt;
use turbomcp_core::{Identity, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, McpRequest};
use turbomcp_telemetry::{SpanPolicy, TraceContextLayer};

/// Inner service: succeeds, echoing an empty result.
#[derive(Clone)]
struct Inner;

impl Service<McpRequest> for Inner {
    type Response = Option<JsonRpcMessage>;
    type Error = Infallible;
    type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _: &mut TaskContext<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: McpRequest) -> Self::Future {
        let reply = match req.message {
            JsonRpcMessage::Request(r) => Some(JsonRpcResponse::success(r.id, json!({})).into()),
            _ => None,
        };
        std::future::ready(Ok(reply))
    }
}

/// Captures every span field (at creation and via later `record`s) as strings.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<HashMap<String, String>>>);

struct FieldVisitor<'a>(&'a Capture);

impl Visit for FieldVisitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0
            .0
            .lock()
            .unwrap()
            .insert(field.name().to_owned(), format!("{value:?}"));
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0
            .0
            .lock()
            .unwrap()
            .insert(field.name().to_owned(), value.to_owned());
    }
}

impl<S> tracing_subscriber::Layer<S> for Capture
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        _id: &tracing::span::Id,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        attrs.record(&mut FieldVisitor(self));
    }

    fn on_record(
        &self,
        _id: &tracing::span::Id,
        values: &tracing::span::Record<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        values.record(&mut FieldVisitor(self));
    }
}

/// A request whose transport authenticated alice, the way HTTP attaches it.
fn identity_request() -> McpRequest {
    let claims = json!({ "email": "a@b.example", "scope": "read" });
    McpRequest::new(JsonRpcRequest::new(1, "tools/call", None)).with(Identity::Bearer {
        sub: "alice".into(),
        claims: claims.as_object().unwrap().clone(),
    })
}

async fn drive(policy: SpanPolicy, req: impl Into<McpRequest>) -> HashMap<String, String> {
    let capture = Capture::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());
    let _guard = tracing::subscriber::set_default(subscriber);
    let svc = TraceContextLayer::with_policy(policy).layer(Inner);
    svc.oneshot(req.into()).await.unwrap();
    let fields = capture.0.lock().unwrap();
    fields.clone()
}

/// Default policy: the subject is a stable hash (never "alice"), claim KEYS
/// are listed, and no recorded field anywhere contains a claim VALUE.
#[tokio::test]
async fn identity_fields_are_redacted_by_default() {
    let fields = drive(SpanPolicy::default(), identity_request()).await;

    let sub = fields.get("mcp.identity.sub").expect("subject recorded");
    assert!(sub.starts_with("sub:"), "hashed form, got {sub}");
    let claims = fields.get("mcp.identity.claims").expect("claim keys");
    assert!(
        claims.contains("email") && claims.contains("scope"),
        "{claims}"
    );
    for (name, value) in &fields {
        assert!(
            !value.contains("alice"),
            "raw subject leaked into {name}: {value}"
        );
        assert!(
            !value.contains("a@b.example"),
            "claim value leaked into {name}: {value}"
        );
    }
}

/// `unredacted()` opts into the raw subject — but claim VALUES still never
/// appear (only the keys view exists at all).
#[tokio::test]
async fn unredacted_policy_records_the_raw_subject_but_never_claim_values() {
    let fields = drive(SpanPolicy::unredacted(), identity_request()).await;

    assert_eq!(
        fields.get("mcp.identity.sub").map(String::as_str),
        Some("alice")
    );
    for (name, value) in &fields {
        assert!(
            !value.contains("a@b.example"),
            "claim value leaked into {name}: {value}"
        );
    }
}

/// An anonymous request records no identity fields at all.
#[tokio::test]
async fn anonymous_requests_record_no_identity_fields() {
    let req: JsonRpcMessage = JsonRpcRequest::new(1, "ping", None).into();
    let fields = drive(SpanPolicy::default(), req).await;
    assert!(!fields.contains_key("mcp.identity.sub"));
    assert!(!fields.contains_key("mcp.identity.claims"));
}

/// The exported span continues the caller's distributed trace: same trace id,
/// parented to the caller's span id (`set_parent`, not just `extract`).
#[tokio::test]
async fn spans_continue_the_callers_trace() {
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};

    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let otel = tracing_opentelemetry::layer().with_tracer(provider.tracer("test"));
    let subscriber = tracing_subscriber::registry().with(otel);
    let _guard = tracing::subscriber::set_default(subscriber);

    let svc = TraceContextLayer::new().layer(Inner);
    let req: JsonRpcMessage = JsonRpcRequest::new(
        1,
        "tools/call",
        Some(json!({ "_meta": {
            "traceparent": "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
        }})),
    )
    .into();
    svc.oneshot(req.into()).await.unwrap();
    provider.force_flush().unwrap();

    let spans = exporter.get_finished_spans().unwrap();
    let span = spans
        .iter()
        .find(|s| s.name == "tools/call")
        .expect("the request span exported");
    assert_eq!(
        span.span_context.trace_id().to_string(),
        "0af7651916cd43dd8448eb211c80319c",
        "same trace id as the caller"
    );
    assert_eq!(
        span.parent_span_id.to_string(),
        "b7ad6b7169203331",
        "parented to the caller's span"
    );
}

/// `tracestate` and `baggage` survive an extract → inject roundtrip alongside
/// `traceparent` (the propagation module runs BOTH standard propagators).
#[test]
fn tracestate_and_baggage_survive_the_roundtrip() {
    let mut meta = Map::new();
    meta.insert(
        "traceparent".into(),
        json!("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"),
    );
    meta.insert("tracestate".into(), json!("vendor=x,other=y"));
    meta.insert("baggage".into(), json!("tenant=t-42"));

    let cx = turbomcp_telemetry::extract_context(&meta);
    let mut out = Map::new();
    turbomcp_telemetry::inject_context(&cx, &mut out);

    assert_eq!(
        out["traceparent"].as_str().unwrap(),
        "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"
    );
    assert_eq!(out["tracestate"], json!("vendor=x,other=y"));
    assert_eq!(
        out.get("baggage").and_then(Value::as_str),
        Some("tenant=t-42")
    );
}
