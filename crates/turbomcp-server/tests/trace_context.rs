//! The dispatcher lifts W3C trace context from a request's `_meta` into
//! `RequestContext.trace_context` (the handler-facing half of SEP-414
//! propagation) — on both protocol versions.

use serde_json::{Value, json};
use tower::{Service, ServiceExt};
use turbomcp_core::{Implementation, JsonRpcMessage, JsonRpcRequest, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, ListToolsContext, McpServerCore, MethodRouter, VersionDispatcher, WithTools,
};

const TRACEPARENT: &str = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";

/// A tool that reports the `traceparent` it observed on the request context
/// (or `"none"`), proving trace context reached the handler.
#[derive(Clone)]
struct TraceEcho;

impl McpServerCore for TraceEcho {
    fn server_info(&self) -> Implementation {
        Implementation::new("trace-echo", "0.1.0")
    }
}

impl WithTools for TraceEcho {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![]))
    }

    async fn call_tool(
        &self,
        ctx: &CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        let seen = ctx
            .base
            .trace_context
            .as_ref()
            .map_or_else(|| "none".to_owned(), |tc| tc.traceparent.clone());
        Ok(neutral::CallToolResult::text(seen))
    }
}

async fn call_with_meta(meta: Value) -> String {
    let mut dispatcher = VersionDispatcher::new(TraceEcho, MethodRouter::new().with_tools());
    let req: JsonRpcMessage = JsonRpcRequest::new(
        1,
        "tools/call",
        Some(json!({ "name": "trace-echo", "arguments": {}, "_meta": meta })),
    )
    .into();
    let resp = dispatcher.ready().await.unwrap().call(req).await.unwrap();
    let Some(JsonRpcMessage::Response(r)) = resp else {
        panic!("expected a response, got {resp:?}");
    };
    let result = r.result.expect("ok result");
    result["content"][0]["text"]
        .as_str()
        .expect("text content")
        .to_owned()
}

#[tokio::test]
async fn draft_request_propagates_trace_context_to_handler() {
    let seen = call_with_meta(json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
        "traceparent": TRACEPARENT,
        "tracestate": "vendor=abc",
    }))
    .await;
    assert_eq!(seen, TRACEPARENT);
}

#[tokio::test]
async fn request_without_traceparent_has_no_trace_context() {
    let seen = call_with_meta(json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
    }))
    .await;
    assert_eq!(seen, "none");
}
