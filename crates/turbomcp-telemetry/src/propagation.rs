//! Trace context propagation over MCP `_meta`.
//!
//! MCP carries distributed-tracing context in a request's `params._meta` (the
//! `traceparent`/`tracestate`/`baggage` keys, SEP-414) rather than HTTP headers,
//! so the same propagation works across stdio, HTTP, and WS. These adapters
//! bridge an MCP `_meta` object to the OpenTelemetry `Extractor`/`Injector`
//! interfaces.
//!
//! "Instrumentations SHOULD propagate context using the configured
//! OpenTelemetry propagators", so both directions use the global one
//! (`opentelemetry::global::set_text_map_propagator`): a deployment on B3,
//! Jaeger or X-Ray propagation gets it here too. With none configured (the
//! global default propagates nothing), they fall back to the W3C pair, trace
//! context and baggage.

use opentelemetry::Context;
use opentelemetry::global;
use opentelemetry::propagation::{
    Extractor, Injector, TextMapCompositePropagator, TextMapPropagator,
};
use opentelemetry_sdk::propagation::{BaggagePropagator, TraceContextPropagator};
use serde_json::{Map, Value};

/// Adapts an MCP `_meta` object to the OTel [`Extractor`] interface (string
/// values only — the W3C keys are all strings).
struct MetaExtractor<'a>(&'a Map<String, Value>);

impl Extractor for MetaExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(Value::as_str)
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(String::as_str).collect()
    }
}

/// Adapts an MCP `_meta` object to the OTel [`Injector`] interface.
struct MetaInjector<'a>(&'a mut Map<String, Value>);

impl Injector for MetaInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        self.0.insert(key.to_owned(), Value::String(value));
    }
}

/// The W3C pair: trace context and baggage. What `init_otlp` installs
/// globally, and what extraction and injection use when nothing is.
#[must_use]
pub fn w3c_propagator() -> TextMapCompositePropagator {
    TextMapCompositePropagator::new(vec![
        Box::new(TraceContextPropagator::new()),
        Box::new(BaggagePropagator::new()),
    ])
}

/// Run `f` with the configured global propagator, or the W3C pair when the
/// global one is the default no-op (it names no fields).
fn with_propagator<T>(f: impl FnOnce(&dyn TextMapPropagator) -> T) -> T {
    let mut f = Some(f);
    let configured = global::get_text_map_propagator(|p| {
        p.fields()
            .next()
            .is_some()
            .then(|| (f.take().expect("called once"))(p))
    });
    match configured {
        Some(out) => out,
        None => (f.take().expect("not yet called"))(&w3c_propagator()),
    }
}

/// Extract a parent [`Context`] from an MCP `_meta` object: the trace context
/// and baggage the caller propagated. An empty or trace-context-less `_meta`
/// yields the default (root) context, so the server starts a fresh trace.
#[must_use]
pub fn extract(meta: &Map<String, Value>) -> Context {
    let extractor = MetaExtractor(meta);
    with_propagator(|p| p.extract(&extractor))
}

/// Inject the trace context and baggage from `cx` into an MCP `_meta` object,
/// so the request it goes out on continues the trace.
pub fn inject(cx: &Context, meta: &mut Map<String, Value>) {
    let mut injector = MetaInjector(meta);
    with_propagator(|p| p.inject_context(cx, &mut injector));
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::trace::TraceContextExt;
    use serde_json::json;

    #[test]
    fn extracts_traceparent_into_parent_context() {
        // A valid W3C traceparent: version-traceid-spanid-flags.
        let mut meta = Map::new();
        meta.insert(
            "traceparent".into(),
            json!("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"),
        );
        let cx = extract(&meta);
        let span = cx.span();
        let sc = span.span_context();
        assert!(sc.is_valid());
        assert_eq!(
            format!("{:032x}", sc.trace_id()),
            "0af7651916cd43dd8448eb211c80319c"
        );
    }

    #[test]
    fn empty_meta_yields_invalid_root_context() {
        let cx = extract(&Map::new());
        assert!(!cx.span().span_context().is_valid());
    }

    #[test]
    fn inject_then_extract_roundtrips_the_trace_id() {
        let mut meta = Map::new();
        meta.insert(
            "traceparent".into(),
            json!("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"),
        );
        let cx = extract(&meta);

        let mut out = Map::new();
        inject(&cx, &mut out);
        assert!(out.contains_key("traceparent"));
        // The injected context re-extracts to the same trace id.
        let again = extract(&out);
        assert_eq!(
            again.span().span_context().trace_id(),
            cx.span().span_context().trace_id(),
        );
    }
}
