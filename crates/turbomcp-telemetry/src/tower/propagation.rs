//! W3C trace-context extraction for the telemetry middleware
//!
//! MCP carries trace context in `params._meta` under the `traceparent` and
//! `tracestate` keys (the convention the OpenTelemetry semantic conventions
//! for MCP use, and which later MCP revisions reserve); plain HTTP carries it
//! in headers of the same names. Either way the remote context becomes the
//! parent of the `mcp.request` span, so the server's spans join the caller's
//! trace instead of starting a new one.

use opentelemetry::propagation::{Extractor, TextMapPropagator};
use opentelemetry::trace::TraceContextExt;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;

/// Reads trace context from a JSON-RPC request's `params._meta` object.
pub(super) struct MetaExtractor<'a>(Option<&'a serde_json::Map<String, serde_json::Value>>);

impl<'a> MetaExtractor<'a> {
    pub(super) fn new(req: &'a serde_json::Value) -> Self {
        Self(
            req.get("params")
                .and_then(|p| p.get("_meta"))
                .and_then(serde_json::Value::as_object),
        )
    }
}

impl Extractor for MetaExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0?.get(key)?.as_str()
    }

    fn keys(&self) -> Vec<&str> {
        self.0
            .map(|meta| meta.keys().map(String::as_str).collect())
            .unwrap_or_default()
    }
}

/// Reads trace context from HTTP request headers.
pub(super) struct HeaderExtractor<'a>(pub(super) &'a http::HeaderMap);

impl Extractor for HeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key)?.to_str().ok()
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(http::HeaderName::as_str).collect()
    }
}

/// Make the trace context found in `carrier`, if any, the parent of `span`.
///
/// Must run before `span` is first entered: tracing-opentelemetry fixes a
/// span's parent when the OpenTelemetry span starts.
pub(super) fn adopt_remote_parent(span: &Span, carrier: &dyn Extractor) {
    let cx = TraceContextPropagator::new().extract(carrier);
    // A missing or malformed `traceparent` extracts to an empty context.
    // Setting that as the parent would detach the span from its local parent
    // (an enclosing HTTP span, say), so only adopt a real remote context.
    if !cx.span().span_context().is_valid() {
        return;
    }
    // This only fails when the subscriber has no tracing-opentelemetry layer
    // or the span is disabled; either way there is no trace to join.
    let _ = span.set_parent(cx);
}
