//! End to end: a client with `ClientTelemetry` calling a server behind
//! `TraceContextLayer` produces one trace, the server's span a child of the
//! client's (the context travels in the request's `_meta`).
#![cfg(feature = "client")]

use std::sync::Arc;

use opentelemetry::trace::{SpanKind, TracerProvider as _};
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};
use serde_json::{Map, json};
use tokio::io::{BufReader, split};
use tower::Layer as _;
use tracing_subscriber::layer::SubscriberExt;
use turbomcp_client::{ClientBuilder, ConnectMode};
use turbomcp_core::codec::DefaultCodec;
use turbomcp_core::{Implementation, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, ListToolsContext, McpServerCore, MethodRouter, VersionDispatcher, WithTools,
};
use turbomcp_service::io::LineTransport;
use turbomcp_telemetry::{ClientTelemetry, TraceContextLayer};

#[derive(Clone)]
struct Adder;

impl McpServerCore for Adder {
    fn server_info(&self) -> Implementation {
        Implementation::new("adder", "1.0.0")
    }
}

impl WithTools for Adder {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![neutral::Tool::new(
            "add",
            json!({ "type": "object" }),
        )]))
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        Ok(neutral::CallToolResult::text("3"))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_servers_span_is_a_child_of_the_clients() {
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let otel = tracing_opentelemetry::layer().with_tracer(provider.tracer("test"));
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(otel))
        .expect("the only subscriber in this test binary");

    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let (s_rd, s_wr) = split(server_io);
    let service = TraceContextLayer::new().layer(VersionDispatcher::new(
        Adder,
        MethodRouter::new().with_tools(),
    ));
    tokio::spawn(turbomcp_service::serve(
        LineTransport::new(BufReader::new(s_rd), s_wr, DefaultCodec::default()),
        service,
    ));
    let (c_rd, c_wr) = split(client_io);
    let client = ClientBuilder::new("agent", "1.0.0")
        .with_connect_mode(ConnectMode::Modern)
        .with_observer(Arc::new(ClientTelemetry::new()))
        .connect(LineTransport::new(
            BufReader::new(c_rd),
            c_wr,
            DefaultCodec::default(),
        ))
        .await
        .expect("connect");

    let result = client.call_tool("add", Map::new()).await.expect("call");
    assert_eq!(result.text_content().as_deref(), Some("3"));
    provider.force_flush().unwrap();

    let spans = exporter.get_finished_spans().unwrap();
    let find = |kind: SpanKind| {
        spans
            .iter()
            .find(|s| s.name == "tools/call add" && s.span_kind == kind)
            .unwrap_or_else(|| panic!("a {kind:?} span for the call in {spans:#?}"))
    };
    let client_span = find(SpanKind::Client);
    let server_span = find(SpanKind::Server);
    assert_eq!(
        server_span.span_context.trace_id(),
        client_span.span_context.trace_id(),
        "one trace"
    );
    assert_eq!(
        server_span.parent_span_id,
        client_span.span_context.span_id(),
        "the server's span is the client's child"
    );
    // Both ends name the transport: stdio is a pipe, with no protocol.
    for span in [client_span, server_span] {
        let transport = span
            .attributes
            .iter()
            .find(|kv| kv.key.as_str() == "network.transport")
            .map(|kv| kv.value.as_str().into_owned());
        assert_eq!(transport.as_deref(), Some("pipe"), "{span:#?}");
        assert!(
            !span
                .attributes
                .iter()
                .any(|kv| kv.key.as_str() == "network.protocol.name")
        );
    }
}
