//! [`TraceContextLayer`] — a [`tower::Layer`] that wraps each MCP request in an
//! OpenTelemetry server span, named and attributed by the MCP semantic
//! conventions.
//!
//! It is transport-agnostic (it sees `JsonRpcMessage`, like every shared RPC
//! layer). For each request it:
//!
//! - names the span `{mcp.method.name} {target}` (`tools/call add`), with
//!   `mcp.method.name`, `mcp.protocol.version`, `jsonrpc.request.id`, the
//!   target's `gen_ai.tool.name` / `gen_ai.prompt.name` / `mcp.resource.uri`,
//!   and `gen_ai.operation.name = execute_tool` on tool calls;
//! - parents it to the context the caller propagated in `_meta`
//!   (`traceparent`, `tracestate`, `baggage`), linking whatever span was
//!   current when the request arrived;
//! - when the request finishes, records `rpc.response.status_code`,
//!   `error.type` and an `ERROR` status as the conventions classify the
//!   outcome (`-32601`, `-32602` and friends are the caller's mistakes, not
//!   failures; an `isError` tool result is `tool_error`);
//! - records the caller's subject and the session id only as keyed hashes
//!   (see [`RedactionKey`](crate::RedactionKey)), and claim *keys*, never
//!   their values.
//!
//! With a `tracing` subscriber carrying the `tracing-opentelemetry` layer (see
//! the `otlp` module), these spans export as OTLP.

use std::pin::Pin;
use std::task::{Context, Poll};

use opentelemetry::trace::TraceContextExt as _;
use pin_project_lite::pin_project;
use tower::{Layer, Service};
use tracing::Instrument;
use tracing::field::Empty;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use turbomcp_core::{Identity, JsonRpcMessage, McpRequest, SessionId};

use crate::SpanPolicy;
use crate::propagation;
use crate::semconv::{self, Outcome, Target};

/// A [`tower::Layer`] that wraps each RPC in an OpenTelemetry server span
/// following the MCP semantic conventions, continuing the caller's trace from
/// `_meta`. Compose it around a dispatcher like any shared RPC middleware.
#[derive(Debug, Clone, Default)]
pub struct TraceContextLayer {
    policy: SpanPolicy,
    extra_methods: std::sync::Arc<[String]>,
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
        Self {
            policy,
            ..Self::default()
        }
    }

    /// Name these methods in `mcp.method.name` too, alongside the spec's own
    /// (an extension's methods, say). Anything else is `_OTHER`.
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

impl<S> Layer<S> for TraceContextLayer {
    type Service = TraceContextService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TraceContextService {
            inner,
            policy: self.policy,
            extra_methods: std::sync::Arc::clone(&self.extra_methods),
        }
    }
}

/// The service produced by [`TraceContextLayer`].
#[derive(Debug, Clone)]
pub struct TraceContextService<S> {
    inner: S,
    policy: SpanPolicy,
    extra_methods: std::sync::Arc<[String]>,
}

impl<S, E> Service<McpRequest> for TraceContextService<S>
where
    S: Service<McpRequest, Response = Option<JsonRpcMessage>, Error = E>,
{
    type Response = Option<JsonRpcMessage>;
    type Error = E;
    type Future = TracedFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: McpRequest) -> Self::Future {
        let span = self.make_span(&req);
        TracedFuture {
            inner: self.inner.call(req).instrument(span.clone()),
            span,
        }
    }
}

pin_project! {
    /// Runs the request inside its span and records how it ended.
    pub struct TracedFuture<F> {
        #[pin]
        inner: tracing::instrument::Instrumented<F>,
        span: tracing::Span,
    }
}

impl<F, E> Future for TracedFuture<F>
where
    F: Future<Output = Result<Option<JsonRpcMessage>, E>>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let result = std::task::ready!(this.inner.poll(cx));
        let outcome = Outcome::of(&result);
        if let Some(code) = outcome.status_code {
            this.span
                .record(semconv::RPC_RESPONSE_STATUS_CODE, code.to_string());
        }
        if let Some(error_type) = &outcome.error_type {
            this.span.record(semconv::ERROR_TYPE, error_type.as_ref());
            this.span.record("otel.status_code", "ERROR");
            // "If the span status is set to `ERROR`, the status description
            // SHOULD match the `JSONRPCError.message`."
            if let Some(message) = &outcome.message {
                this.span
                    .record("otel.status_description", message.as_str());
            }
        }
        Poll::Ready(result)
    }
}

impl<S> TraceContextService<S> {
    fn make_span(&self, req: &McpRequest) -> tracing::Span {
        let msg = &req.message;
        let method = msg
            .method()
            .map_or(std::borrow::Cow::Borrowed("(response)"), |m| {
                semconv::method_label(m, &self.extra_methods)
            });
        let target = Target::of(msg);
        // "Span name SHOULD follow the format `{mcp.method.name} {target}`."
        let name = match target.as_ref().and_then(Target::span_suffix) {
            Some(suffix) => format!("{method} {suffix}"),
            None => method.to_string(),
        };
        let span = tracing::info_span!(
            "mcp.server",
            otel.name = name.as_str(),
            otel.kind = "server",
            otel.status_code = Empty,
            otel.status_description = Empty,
            "mcp.method.name" = method.as_ref(),
            "mcp.protocol.version" = Empty,
            "mcp.session.id" = Empty,
            "mcp.resource.uri" = Empty,
            "jsonrpc.request.id" = Empty,
            "gen_ai.tool.name" = Empty,
            "gen_ai.prompt.name" = Empty,
            "gen_ai.operation.name" = Empty,
            "error.type" = Empty,
            "rpc.response.status_code" = Empty,
            mcp.identity.sub = Empty,
            mcp.identity.claims = Empty,
        );

        if let JsonRpcMessage::Request(r) = msg {
            let id = match &r.id {
                turbomcp_core::RequestId::Number(n) => n.to_string(),
                turbomcp_core::RequestId::String(s) => s.clone(),
            };
            span.record(semconv::JSONRPC_REQUEST_ID, id);
        }
        if let Some(version) = semconv::protocol_version(msg) {
            span.record(semconv::MCP_PROTOCOL_VERSION, version);
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
        // A session id is a bearer-equivalent handle (the transports spec
        // says to treat it as a secret), so the attribute carries its keyed
        // hash: enough to group a session's requests, useless to replay.
        if let Some(session) = req.extensions.get::<SessionId>() {
            span.record(
                semconv::MCP_SESSION_ID,
                self.policy.key.redact("session", session.as_str()),
            );
        }

        // "MCP server instrumentation SHOULD, by default, use context
        // extracted from MCP `params._meta` as a parent for MCP server span
        // and SHOULD link current ambient context, if it's present."
        // `set_parent` errors only when no `tracing-opentelemetry` layer is
        // installed, which leaves nothing to parent.
        if let Some(meta) = request_meta(msg) {
            let parent = propagation::extract(meta);
            if parent.span().span_context().is_valid() {
                let ambient = tracing::Span::current().context();
                let ambient = ambient.span().span_context().clone();
                let _ = span.set_parent(parent);
                if ambient.is_valid() {
                    span.add_link(ambient);
                }
            }
        }

        if let Some(identity) = req
            .extensions
            .get::<Identity>()
            .filter(|i| i.is_authenticated())
        {
            if let Some(sub) = identity.subject() {
                if self.policy.redact_subject {
                    span.record("mcp.identity.sub", self.policy.key.redact("sub", sub));
                } else {
                    span.record("mcp.identity.sub", sub);
                }
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
        }));
        let alice = Identity::Bearer {
            sub: "alice".into(),
            claims: json!({ "scope": "read" }).as_object().unwrap().clone(),
        };
        let resp = svc.oneshot(McpRequest::new(req).with(alice)).await.unwrap();
        assert!(matches!(resp, Some(JsonRpcMessage::Response(_))));
    }

    #[tokio::test]
    async fn handles_request_without_meta() {
        let svc = TraceContextLayer::new().layer(Inner);
        let req: JsonRpcMessage = JsonRpcRequest::new(1, "ping", None).into();
        let resp = svc.oneshot(req.into()).await.unwrap();
        assert!(matches!(resp, Some(JsonRpcMessage::Response(_))));
    }

    #[test]
    fn composes_as_mcp_service() {
        // The layer over a ProtocolError service is still an McpService.
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
        assert_mcp_service(&TraceContextLayer::new().layer(Dispatcher));
    }
}
