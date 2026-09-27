//! [`TraceContextLayer`] — a [`tower::Layer`] that wraps each MCP request in an
//! OpenTelemetry-aware span.
//!
//! It is transport-agnostic (it sees `JsonRpcMessage`, like every shared RPC
//! layer): for each request it extracts the W3C parent context from `_meta`,
//! opens a span as that trace's child, and records the method plus a
//! **redaction-safe** view of the caller's identity (a hashed subject and the
//! claim *keys* — never claim values). With a `tracing` subscriber carrying the
//! `tracing-opentelemetry` layer (see the `otlp` module), those spans export as
//! OTLP.

use std::task::{Context, Poll};

use tower::{Layer, Service};
use tracing::Instrument;
use tracing::field::Empty;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use turbomcp_core::{JsonRpcMessage, RedactedSubject, meta};

use crate::SpanPolicy;
use crate::propagation;

/// A [`tower::Layer`] that wraps each RPC in an OpenTelemetry span continuing the
/// caller's W3C trace context (from `_meta`) and carrying redacted identity
/// attributes. Compose it around a dispatcher like any shared RPC middleware.
#[derive(Debug, Clone, Copy, Default)]
pub struct TraceContextLayer {
    policy: SpanPolicy,
}

impl TraceContextLayer {
    /// A layer with the default (fully redacted) [`SpanPolicy`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A layer with an explicit identity-recording policy.
    #[must_use]
    pub fn with_policy(policy: SpanPolicy) -> Self {
        Self { policy }
    }
}

impl<S> Layer<S> for TraceContextLayer {
    type Service = TraceContextService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TraceContextService {
            inner,
            policy: self.policy,
        }
    }
}

/// The service produced by [`TraceContextLayer`].
#[derive(Debug, Clone)]
pub struct TraceContextService<S> {
    inner: S,
    policy: SpanPolicy,
}

impl<S, E> Service<JsonRpcMessage> for TraceContextService<S>
where
    S: Service<JsonRpcMessage, Response = Option<JsonRpcMessage>, Error = E>,
{
    type Response = Option<JsonRpcMessage>;
    type Error = E;
    type Future = tracing::instrument::Instrumented<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: JsonRpcMessage) -> Self::Future {
        let span = self.make_span(&req);
        self.inner.call(req).instrument(span)
    }
}

impl<S> TraceContextService<S> {
    /// Build the per-request span: parent it to the caller's extracted trace
    /// context and record method + redacted identity.
    fn make_span(&self, req: &JsonRpcMessage) -> tracing::Span {
        let method = req.method().unwrap_or("(response)");
        // `otel.name` sets the exported span name; `otel.kind` marks it a server
        // span. Identity fields are filled below (Empty until recorded).
        let span = tracing::info_span!(
            "mcp.request",
            otel.name = method,
            otel.kind = "server",
            mcp.method = method,
            mcp.identity.sub = Empty,
            mcp.identity.claims = Empty,
        );

        let Some(meta) = request_meta(req) else {
            return span;
        };

        // Continue the caller's distributed trace, if any. `set_parent` errors
        // only when no `tracing-opentelemetry` layer is installed (no exporter
        // configured) — harmless, the span just has no OTel parent then.
        let _ = span.set_parent(propagation::extract(meta));

        // Redacted identity attributes (PII-safe by default).
        let identity = meta::extract_identity(meta);
        if identity.is_authenticated() {
            if self.policy.redact_subject {
                span.record(
                    "mcp.identity.sub",
                    tracing::field::display(RedactedSubject(&identity)),
                );
            } else if let Some(sub) = identity.subject() {
                span.record("mcp.identity.sub", sub);
            }
            if self.policy.record_claim_keys {
                let keys = identity.claim_keys().join(",");
                if !keys.is_empty() {
                    span.record("mcp.identity.claims", keys.as_str());
                }
            }
        }

        span
    }
}

/// The `params._meta` object of a request/notification, if present.
fn request_meta(req: &JsonRpcMessage) -> Option<&serde_json::Map<String, serde_json::Value>> {
    let params = match req {
        JsonRpcMessage::Request(r) => r.params.as_ref(),
        JsonRpcMessage::Notification(n) => n.params.as_ref(),
        JsonRpcMessage::Response(_) => None,
    };
    params
        .and_then(|p| p.get("_meta"))
        .and_then(serde_json::Value::as_object)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;
    use std::task::Poll;

    use serde_json::json;
    use tower::ServiceExt;
    use turbomcp_core::{JsonRpcRequest, JsonRpcResponse};

    /// Inner service: succeeds, echoing the method.
    #[derive(Clone)]
    struct Inner;

    impl Service<JsonRpcMessage> for Inner {
        type Response = Option<JsonRpcMessage>;
        type Error = Infallible;
        type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

        fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: JsonRpcMessage) -> Self::Future {
            let reply = match req {
                JsonRpcMessage::Request(r) => {
                    Some(JsonRpcResponse::success(r.id, json!({})).into())
                }
                _ => None,
            };
            std::future::ready(Ok(reply))
        }
    }

    fn request_with_meta(meta: serde_json::Value) -> JsonRpcMessage {
        JsonRpcRequest::new(1, "tools/call", Some(json!({ "_meta": meta }))).into()
    }

    #[tokio::test]
    async fn passes_request_through_and_returns_response() {
        let svc = TraceContextLayer::new().layer(Inner);
        let req = request_with_meta(json!({
            "traceparent": "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
            "io.turbomcp.internal/identity": { "sub": "alice", "claims": { "scope": "read" } },
        }));
        let resp = svc.oneshot(req).await.unwrap();
        assert!(matches!(resp, Some(JsonRpcMessage::Response(_))));
    }

    #[tokio::test]
    async fn handles_request_without_meta() {
        let svc = TraceContextLayer::new().layer(Inner);
        let req: JsonRpcMessage = JsonRpcRequest::new(1, "ping", None).into();
        let resp = svc.oneshot(req).await.unwrap();
        assert!(matches!(resp, Some(JsonRpcMessage::Response(_))));
    }

    #[test]
    fn composes_as_mcp_service() {
        // The layer over a ProtocolError service is still an McpService.
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
        assert_mcp_service(&TraceContextLayer::new().layer(Dispatcher));
    }
}
