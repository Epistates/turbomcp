//! Exported spans follow the OpenTelemetry MCP semantic conventions: the
//! name carries the tool or prompt, the attributes are the conventions'
//! names, and the outcome is classified as they say.

use std::convert::Infallible;
use std::task::{Context as TaskContext, Poll};

use opentelemetry::trace::{Status, TracerProvider as _};
use opentelemetry::{KeyValue, Value as OtelValue};
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider, SpanData};
use serde_json::{Value, json};
use tower::{Layer as TowerLayer, Service, ServiceExt};
use tracing_subscriber::layer::SubscriberExt;
use turbomcp_core::{
    JsonRpcError, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, McpRequest, SessionId, codes,
};
use turbomcp_telemetry::TraceContextLayer;

/// Answers with whatever it was built with.
#[derive(Clone)]
enum Answer {
    Result(Value),
    Fail(i32),
}

impl Service<McpRequest> for Answer {
    type Response = Option<JsonRpcMessage>;
    type Error = Infallible;
    type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _: &mut TaskContext<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: McpRequest) -> Self::Future {
        let JsonRpcMessage::Request(r) = req.message else {
            return std::future::ready(Ok(None));
        };
        let reply = match self {
            Answer::Result(v) => JsonRpcResponse::success(r.id, v.clone()),
            Answer::Fail(code) => JsonRpcResponse::error(
                r.id,
                JsonRpcError {
                    code: *code,
                    message: "it broke".into(),
                    data: None,
                },
            ),
        };
        std::future::ready(Ok(Some(reply.into())))
    }
}

async fn export(answer: Answer, req: McpRequest) -> SpanData {
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let otel = tracing_opentelemetry::layer().with_tracer(provider.tracer("test"));
    let subscriber = tracing_subscriber::registry().with(otel);
    let _guard = tracing::subscriber::set_default(subscriber);
    TraceContextLayer::new()
        .layer(answer)
        .oneshot(req)
        .await
        .unwrap();
    provider.force_flush().unwrap();
    exporter
        .get_finished_spans()
        .unwrap()
        .pop()
        .expect("a span")
}

fn attr<'a>(span: &'a SpanData, key: &str) -> Option<&'a OtelValue> {
    span.attributes
        .iter()
        .find(|kv: &&KeyValue| kv.key.as_str() == key)
        .map(|kv| &kv.value)
}

fn attr_str(span: &SpanData, key: &str) -> Option<String> {
    attr(span, key).map(|v| v.as_str().into_owned())
}

fn call(name: &str) -> McpRequest {
    McpRequest::new(JsonRpcRequest::new(
        7,
        "tools/call",
        Some(json!({
            "name": name,
            "_meta": { "io.modelcontextprotocol/protocolVersion": "2025-11-25" },
        })),
    ))
}

#[tokio::test]
async fn a_tool_call_span_is_named_and_attributed_by_the_conventions() {
    let span = export(Answer::Result(json!({ "content": [] })), call("add")).await;
    assert_eq!(span.name, "tools/call add");
    assert_eq!(span.span_kind, opentelemetry::trace::SpanKind::Server);
    assert_eq!(
        attr_str(&span, "mcp.method.name").as_deref(),
        Some("tools/call")
    );
    assert_eq!(attr_str(&span, "gen_ai.tool.name").as_deref(), Some("add"));
    assert_eq!(
        attr_str(&span, "gen_ai.operation.name").as_deref(),
        Some("execute_tool")
    );
    assert_eq!(attr_str(&span, "jsonrpc.request.id").as_deref(), Some("7"));
    assert_eq!(
        attr_str(&span, "mcp.protocol.version").as_deref(),
        Some("2025-11-25")
    );
    assert!(attr(&span, "error.type").is_none());
    assert_eq!(span.status, Status::Unset);
}

/// "If the span status is set to `ERROR`, the status description SHOULD match
/// the `JSONRPCError.message`."
#[tokio::test]
async fn a_server_error_is_an_error_with_its_message() {
    let span = export(Answer::Fail(codes::INTERNAL_ERROR), call("add")).await;
    assert_eq!(attr_str(&span, "error.type").as_deref(), Some("-32603"));
    assert_eq!(
        attr_str(&span, "rpc.response.status_code").as_deref(),
        Some("-32603")
    );
    assert_eq!(span.status, Status::error("it broke"));
}

/// The caller's mistakes (`-32602` among them) are not errors, but the code
/// is still recorded.
#[tokio::test]
async fn invalid_params_is_not_an_error() {
    let span = export(Answer::Fail(codes::INVALID_PARAMS), call("nope")).await;
    assert!(attr(&span, "error.type").is_none());
    assert_eq!(
        attr_str(&span, "rpc.response.status_code").as_deref(),
        Some("-32602")
    );
    assert_eq!(span.status, Status::Unset);
}

/// "When `CallToolResult` returns `isError: true`, set `error.type` to
/// `tool_error`."
#[tokio::test]
async fn a_tool_error_result_is_a_tool_error() {
    let span = export(
        Answer::Result(json!({ "content": [], "isError": true })),
        call("add"),
    )
    .await;
    assert_eq!(attr_str(&span, "error.type").as_deref(), Some("tool_error"));
    assert!(matches!(span.status, Status::Error { .. }));
}

/// A session id is a bearer-equivalent secret: the attribute carries a keyed
/// hash, never the id.
#[tokio::test]
async fn the_session_id_is_recorded_hashed() {
    let req = call("add").with(SessionId::new("super-secret-session"));
    let span = export(Answer::Result(json!({ "content": [] })), req).await;
    let session = attr_str(&span, "mcp.session.id").expect("recorded");
    assert!(session.starts_with("session:"), "{session}");
    assert!(!session.contains("super-secret"));
}

#[tokio::test]
async fn a_resource_read_records_its_uri_but_keeps_it_out_of_the_name() {
    let req = McpRequest::new(JsonRpcRequest::new(
        1,
        "resources/read",
        Some(json!({ "uri": "file:///etc/hosts" })),
    ));
    let span = export(Answer::Result(json!({ "contents": [] })), req).await;
    assert_eq!(span.name, "resources/read");
    assert_eq!(
        attr_str(&span, "mcp.resource.uri").as_deref(),
        Some("file:///etc/hosts")
    );
}
