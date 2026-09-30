//! The typed client methods the transport-twin suites skip — `complete()`,
//! `list_resources()`, `list_resource_templates()` — plus pagination-cursor
//! plumbing and JSON-RPC error mapping, decoded through BOTH negotiated wires
//! (Modern `2026-07-28` and Legacy `2025-11-25`) against a real
//! `VersionDispatcher` server.

use serde_json::json;
use tokio::io::{BufReader, split};
use turbomcp_client::{Client, ClientBuilder, ClientError, ConnectMode};
use turbomcp_core::codec::DefaultCodec;
use turbomcp_core::{Implementation, McpError, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, CompleteContext, GetPromptContext, LegacySessionAdapter, ListPromptsContext,
    ListResourceTemplatesContext, ListResourcesContext, ListToolsContext, McpServerCore,
    MethodRouter, ReadResourceContext, VersionDispatcher, WithCompletions, WithPrompts,
    WithResources, WithTools,
};
use turbomcp_service::io::LineTransport;

#[derive(Clone)]
struct Kitchen;

impl McpServerCore for Kitchen {
    fn server_info(&self) -> Implementation {
        Implementation::new("kitchen", "1.0.0")
    }
}

impl WithTools for Kitchen {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        // Echo the cursor into the tool name so the test observes the plumbing.
        let name = match params.cursor {
            Some(c) => format!("page-{c}"),
            None => "page-first".into(),
        };
        let mut result = neutral::ListToolsResult::new(vec![neutral::Tool::new(
            name,
            json!({"type": "object"}),
        )]);
        result.next_cursor = Some("next-42".into());
        Ok(result)
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        Ok(neutral::CallToolResult::text("ok"))
    }
}

impl WithResources for Kitchen {
    async fn list_resources(
        &self,
        _ctx: &ListResourcesContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListResourcesResult> {
        Ok(neutral::ListResourcesResult::new(vec![
            neutral::Resource::new("mem://a", "a").with_mime_type("text/plain"),
        ]))
    }

    async fn read_resource(
        &self,
        _ctx: &ReadResourceContext,
        _params: neutral::ReadResourceParams,
    ) -> McpResult<neutral::ReadResourceResult> {
        Ok(neutral::ReadResourceResult::text("mem://a", "hi"))
    }

    async fn list_resource_templates(
        &self,
        _ctx: &ListResourceTemplatesContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListResourceTemplatesResult> {
        Ok(neutral::ListResourceTemplatesResult::new(vec![
            neutral::ResourceTemplate::new("file://{path}", "files"),
        ]))
    }
}

impl WithPrompts for Kitchen {
    async fn list_prompts(
        &self,
        _ctx: &ListPromptsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListPromptsResult> {
        Ok(neutral::ListPromptsResult::new(vec![neutral::Prompt::new(
            "greet",
        )]))
    }

    async fn get_prompt(
        &self,
        _ctx: &GetPromptContext,
        params: neutral::GetPromptParams,
    ) -> McpResult<neutral::GetPromptResult> {
        if params.name != "greet" {
            return Err(McpError::invalid_params(format!(
                "no such prompt: {}",
                params.name
            )));
        }
        Ok(neutral::GetPromptResult::new(vec![
            neutral::PromptMessage::user_text("hello"),
        ]))
    }
}

impl WithCompletions for Kitchen {
    async fn complete(
        &self,
        _ctx: &CompleteContext,
        _params: neutral::CompleteParams,
    ) -> McpResult<neutral::CompleteResult> {
        Ok(neutral::CompleteResult::new(vec![
            "alpha".to_string(),
            "beta".to_string(),
        ]))
    }
}

/// Spawn the Kitchen server (behind the legacy session adapter, so both wire
/// families work) and connect a typed client in `mode`.
async fn connect(mode: ConnectMode) -> Client {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let (s_rd, s_wr) = split(server_io);
    let router = MethodRouter::new()
        .with_tools()
        .with_resources()
        .with_prompts()
        .with_completions();
    let service = LegacySessionAdapter::new(VersionDispatcher::new(Kitchen, router));
    let server_transport = LineTransport::new(BufReader::new(s_rd), s_wr, DefaultCodec::default());
    tokio::spawn(async move {
        let _ = turbomcp_service::serve(server_transport, service).await;
    });

    let (c_rd, c_wr) = split(client_io);
    let transport = LineTransport::new(BufReader::new(c_rd), c_wr, DefaultCodec::default());
    ClientBuilder::new("typed-surface", "1.0.0")
        .with_connect_mode(mode)
        .connect(transport)
        .await
        .expect("handshake succeeds")
}

/// The full typed read surface decodes on both negotiated wires.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn typed_surface_round_trips_on_both_wires() {
    for mode in [ConnectMode::Modern, ConnectMode::Legacy] {
        let client = connect(mode).await;

        let resources = client.list_resources(None).await.expect("list_resources");
        assert_eq!(resources.resources.len(), 1);
        assert_eq!(resources.resources[0].uri, "mem://a");
        assert_eq!(
            resources.resources[0].mime_type.as_deref(),
            Some("text/plain")
        );

        let templates = client
            .list_resource_templates(None)
            .await
            .expect("list_resource_templates");
        assert_eq!(templates.resource_templates.len(), 1);
        assert_eq!(
            templates.resource_templates[0].uri_template,
            "file://{path}"
        );

        let completions = client
            .complete(neutral::CompleteParams::new(
                neutral::CompletionReference::Prompt {
                    name: "greet".into(),
                },
                neutral::CompletionArgument::new("text", "al"),
            ))
            .await
            .expect("complete");
        assert_eq!(completions.values, vec!["alpha", "beta"]);
    }
}

/// The pagination cursor travels client → wire → handler, and the handler's
/// `nextCursor` travels back, on both wires.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pagination_cursor_round_trips_through_the_typed_client() {
    for mode in [ConnectMode::Modern, ConnectMode::Legacy] {
        let client = connect(mode).await;
        let first = client.list_tools(None).await.expect("first page");
        assert_eq!(first.tools[0].name, "page-first");
        assert_eq!(first.next_cursor.as_deref(), Some("next-42"));

        let cont = client.list_tools(Some("p2")).await.expect("continuation");
        assert_eq!(cont.tools[0].name, "page-p2");
    }
}

/// A handler-returned `McpError` surfaces from a typed method as
/// `ClientError::Rpc` with the mapped JSON-RPC code, on both wires.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn handler_errors_surface_as_rpc_errors_through_typed_methods() {
    for mode in [ConnectMode::Modern, ConnectMode::Legacy] {
        let client = connect(mode).await;
        let err = client
            .get_prompt("nope", Default::default())
            .await
            .expect_err("unknown prompt");
        match &err {
            ClientError::Rpc(e) => {
                assert_eq!(e.code, -32602, "invalid_params mapping");
                assert!(e.message.contains("nope"), "{}", e.message);
            }
            other => panic!("expected Rpc, got {other:?}"),
        }
    }
}
