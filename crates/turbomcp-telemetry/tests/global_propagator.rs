//! Propagation uses the globally configured propagator ("SHOULD propagate
//! context using the configured OpenTelemetry propagators"). Its own process,
//! because it replaces the global.

use opentelemetry::propagation::{Extractor, Injector, TextMapPropagator};
use opentelemetry::trace::{SpanContext, SpanId, TraceContextExt, TraceFlags, TraceId, TraceState};
use opentelemetry::{Context, global};
use serde_json::{Map, json};
use turbomcp_telemetry::{extract_context, inject_context};

/// Carries a trace id in a single `x-trace` key, as some vendor format might.
#[derive(Debug)]
struct VendorPropagator {
    fields: Vec<String>,
}

impl TextMapPropagator for VendorPropagator {
    fn inject_context(&self, cx: &Context, injector: &mut dyn Injector) {
        let span = cx.span();
        let sc = span.span_context();
        if sc.is_valid() {
            injector.set("x-trace", format!("{}", sc.trace_id()));
        }
    }

    fn extract_with_context(&self, cx: &Context, extractor: &dyn Extractor) -> Context {
        let Some(trace_id) = extractor
            .get("x-trace")
            .and_then(|v| TraceId::from_hex(v).ok())
        else {
            return cx.clone();
        };
        cx.with_remote_span_context(SpanContext::new(
            trace_id,
            SpanId::from_hex("b7ad6b7169203331").unwrap(),
            TraceFlags::SAMPLED,
            true,
            TraceState::default(),
        ))
    }

    fn fields(&self) -> opentelemetry::propagation::text_map_propagator::FieldIter<'_> {
        opentelemetry::propagation::text_map_propagator::FieldIter::new(&self.fields)
    }
}

#[test]
fn extraction_and_injection_use_the_configured_propagator() {
    global::set_text_map_propagator(VendorPropagator {
        fields: vec!["x-trace".to_owned()],
    });

    let mut vendor = Map::new();
    vendor.insert("x-trace".into(), json!("0af7651916cd43dd8448eb211c80319c"));
    let cx = extract_context(&vendor);
    assert_eq!(
        cx.span().span_context().trace_id().to_string(),
        "0af7651916cd43dd8448eb211c80319c",
        "the vendor key was read"
    );

    let mut w3c = Map::new();
    w3c.insert(
        "traceparent".into(),
        json!("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"),
    );
    assert!(
        !extract_context(&w3c).span().span_context().is_valid(),
        "W3C is not consulted once a propagator is configured"
    );

    let mut out = Map::new();
    inject_context(&cx, &mut out);
    assert!(out.contains_key("x-trace") && !out.contains_key("traceparent"));
}
