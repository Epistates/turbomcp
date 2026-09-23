//! What crosses the gRPC wire between `McpGrpcClient` and `McpGrpcServer`.
//!
//! Each test stands up a real tonic server on a loopback port and talks to it
//! through the real client, so a field the proto or the conversions lose shows
//! up here the way a user would see it.

use std::future::Future;
use std::pin::Pin;

use serde_json::json;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use turbomcp_grpc::client::{McpGrpcClient, McpGrpcClientConfig};
use turbomcp_grpc::server::ToolHandler;
use turbomcp_grpc::{GrpcResult, McpGrpcServer};
use turbomcp_protocol::types::CallToolResult;
use turbomcp_types::{Content, Tool, ToolAnnotations, ToolInputSchema};

/// Serve `server` on an ephemeral loopback port and return its URL.
async fn spawn(server: McpGrpcServer) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(server.into_service())
            .serve_with_incoming(TcpListenerStream::new(listener)),
    );
    format!("http://{addr}")
}

async fn connect_requesting(url: &str, protocol_version: &str) -> McpGrpcClient {
    McpGrpcClient::connect_with_config(
        url,
        McpGrpcClientConfig {
            protocol_version: protocol_version.to_string(),
            ..McpGrpcClientConfig::default()
        },
    )
    .await
    .expect("connect")
}

// =============================================================================
// Version negotiation
// =============================================================================

#[tokio::test]
async fn initialize_echoes_a_supported_older_version() {
    let url = spawn(McpGrpcServer::builder().build()).await;
    let mut client = connect_requesting(&url, "2025-06-18").await;

    let result = client.initialize().await.expect("initialize");

    // Lifecycle: a supported requested version MUST be echoed back. This used
    // to answer 2025-11-25 regardless of what the client asked for.
    assert_eq!(result.protocol_version.as_str(), "2025-06-18");
    assert_eq!(client.protocol_version(), "2025-06-18");
}

#[tokio::test]
async fn initialize_answers_an_unsupported_version_with_one_it_supports() {
    let url = spawn(McpGrpcServer::builder().build()).await;
    let mut client = connect_requesting(&url, "2024-11-05").await;

    let result = client.initialize().await.expect("initialize");

    assert_eq!(
        result.protocol_version.as_str(),
        turbomcp_protocol::PROTOCOL_VERSION
    );
    // The client records what was negotiated, not what it asked for.
    assert_eq!(
        client.protocol_version(),
        turbomcp_protocol::PROTOCOL_VERSION
    );
}

#[tokio::test]
async fn client_refuses_a_version_it_does_not_support() {
    // A server whose fallback is a version this SDK has never heard of.
    let url = spawn(
        McpGrpcServer::builder()
            .protocol_version("1999-01-01")
            .build(),
    )
    .await;
    let mut client = connect_requesting(&url, "2024-11-05").await;

    let err = client
        .initialize()
        .await
        .expect_err("an unsupported negotiated version must not be accepted");

    assert!(err.to_string().contains("1999-01-01"), "{err}");
    assert!(client.server_info().is_none());
}

// =============================================================================
// Metadata survives the wire
// =============================================================================

struct StructuredResult;

impl ToolHandler for StructuredResult {
    fn call_tool(
        &self,
        _name: &str,
        _arguments: Option<serde_json::Value>,
    ) -> Pin<Box<dyn Future<Output = GrpcResult<CallToolResult>> + Send + '_>> {
        Box::pin(async {
            Ok(CallToolResult {
                content: vec![Content::text("3 hits")],
                is_error: None,
                structured_content: Some(json!({"hits": 3})),
                meta: Some([("requestCost".to_string(), json!(7))].into()),
            })
        })
    }
}

fn annotated_tool() -> Tool {
    Tool {
        name: "search".into(),
        description: Some("Searches the index".into()),
        input_schema: ToolInputSchema::default(),
        title: Some("Search".into()),
        icons: None,
        annotations: Some(ToolAnnotations {
            read_only_hint: Some(true),
            destructive_hint: Some(false),
            idempotent_hint: None,
            open_world_hint: None,
            title: None,
        }),
        execution: None,
        output_schema: Some(
            serde_json::from_value(json!({
                "type": "object",
                "properties": {"hits": {"type": "integer"}},
                "required": ["hits"]
            }))
            .expect("output schema"),
        ),
        meta: Some([("ui".to_string(), json!({"resourceUri": "ui://search"}))].into()),
    }
}

#[tokio::test]
async fn tools_list_carries_hints_output_schema_and_meta() {
    let url = spawn(McpGrpcServer::builder().add_tool(annotated_tool()).build()).await;
    let mut client = connect_requesting(&url, turbomcp_protocol::PROTOCOL_VERSION).await;

    let tools = client.list_tools().await.expect("list tools");

    // A read-only hint that vanishes in transit makes a client treat the tool
    // as destructive; a missing output schema means structured results go
    // unvalidated.
    assert_eq!(tools, vec![annotated_tool()]);
}

#[tokio::test]
async fn tools_call_carries_structured_content_and_meta() {
    let url = spawn(
        McpGrpcServer::builder()
            .add_tool(annotated_tool())
            .tool_handler(StructuredResult)
            .build(),
    )
    .await;
    let mut client = connect_requesting(&url, turbomcp_protocol::PROTOCOL_VERSION).await;

    let result = client.call_tool("search", None).await.expect("call tool");

    assert_eq!(result.structured_content, Some(json!({"hits": 3})));
    assert_eq!(
        result
            .meta
            .as_ref()
            .and_then(|meta| meta.get("requestCost")),
        Some(&json!(7))
    );
}
