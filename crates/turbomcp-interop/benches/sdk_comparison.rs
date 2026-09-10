//! Cross-SDK performance comparison: a steady-state `call_tool("add")`
//! round-trip through turbomcp vs the official Rust SDK (`rmcp`), each SDK
//! driving **both ends** (its own client and server) over an in-process
//! `tokio::io::duplex` pipe.
//!
//! Both sit on the `2025-11-25` protocol version (the shared legacy
//! path) so the comparison is apples-to-apples. The connection +
//! handshake happen once, outside the measured loop; each iteration is one full
//! client→server→client tool call including newline-JSON framing on both sides.
//!
//! This crate is excluded from the workspace (rmcp's dep tree stays out of the
//! main lockfile). Run it directly:
//!   `cd crates/turbomcp-interop && cargo bench --bench sdk_comparison`

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ContentBlock, Implementation, ProtocolVersion,
    ServerCapabilities, ServerInfo,
};
use rmcp::{
    ErrorData as RmcpError, ServerHandler, ServiceExt, object, schemars, tool, tool_handler,
    tool_router,
};

use tokio::io::{BufReader, split};
use turbomcp::client::{Client, ClientBuilder, ConnectMode};
use turbomcp::prelude::*;
use turbomcp::{LegacySessionAdapter, SerdeJsonCodec, serve};
use turbomcp_transport_stdio::LineTransport;

// ---- an rmcp server with one `add` tool (mirrors the interop test) ----------

#[derive(Clone)]
struct RmcpAdder {
    #[allow(dead_code)]
    tool_router: ToolRouter<RmcpAdder>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct AddArgs {
    a: i64,
    b: i64,
}

#[tool_router]
impl RmcpAdder {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Add two integers")]
    fn add(
        &self,
        Parameters(AddArgs { a, b }): Parameters<AddArgs>,
    ) -> Result<CallToolResult, RmcpError> {
        Ok(CallToolResult::success(vec![ContentBlock::text(
            (a + b).to_string(),
        )]))
    }
}

#[tool_handler]
impl ServerHandler for RmcpAdder {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_protocol_version(ProtocolVersion::V_2025_11_25)
    }
}

// ---- a turbomcp server with one `add` tool --------------------------------

#[derive(Clone)]
struct TurboAdder;

#[server(name = "turbo-adder", version = "1.0.0")]
impl TurboAdder {
    /// Add two integers.
    #[tool(description = "Add two integers")]
    async fn add(&self, a: i64, b: i64) -> McpResult<String> {
        Ok((a + b).to_string())
    }
}

/// turbomcp client ↔ turbomcp server, connected and handshaken.
async fn connect_turbomcp() -> Client {
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);

    let (s_rd, s_wr) = split(server_io);
    let transport = LineTransport::new(BufReader::new(s_rd), s_wr, SerdeJsonCodec);
    let service = LegacySessionAdapter::new(TurboAdder.into_server().build());
    tokio::spawn(serve(transport, service));

    let (c_rd, c_wr) = split(client_io);
    let client_transport = LineTransport::new(BufReader::new(c_rd), c_wr, SerdeJsonCodec);
    ClientBuilder::new("bench-client", "1.0.0")
        .with_connect_mode(ConnectMode::Legacy)
        .connect(client_transport)
        .await
        .expect("turbomcp handshake")
}

/// rmcp client ↔ rmcp server, connected and handshaken.
async fn connect_rmcp() -> rmcp::service::RunningService<rmcp::service::RoleClient, ()> {
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);

    let (s_rd, s_wr) = split(server_io);
    tokio::spawn(async move {
        if let Ok(running) = RmcpAdder::new().serve((s_rd, s_wr)).await {
            let _ = running.waiting().await;
        }
    });

    ().serve(client_io).await.expect("rmcp handshake")
}

fn bench_call_tool(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("runtime");

    let turbo = rt.block_on(connect_turbomcp());
    let fixture = rt
        .block_on(turbo.call_tool(
            "add",
            serde_json::from_value(serde_json::json!({"a":2,"b":3})).unwrap(),
        ))
        .expect("TurboMCP fixture");
    assert!(!fixture.is_error);
    assert!(
        matches!(fixture.content.as_slice(), [turbomcp::neutral::Content::Text { text, .. }] if text == "5")
    );
    c.bench_function("turbomcp/call_tool_roundtrip", |b| {
        b.to_async(&rt).iter(|| async {
            let mut args = serde_json::Map::new();
            args.insert("a".into(), serde_json::json!(2));
            args.insert("b".into(), serde_json::json!(3));
            let result = turbo.call_tool("add", args).await.expect("call_tool");
            black_box(result);
        });
    });

    let rmcp_client = rt.block_on(connect_rmcp());
    let fixture = rt
        .block_on(
            rmcp_client.call_tool(
                CallToolRequestParams::new("add").with_arguments(object!({"a":2,"b":3})),
            ),
        )
        .expect("rmcp fixture");
    assert_ne!(fixture.is_error, Some(true));
    assert_eq!(
        serde_json::to_value(&fixture).unwrap()["content"][0]["text"],
        "5"
    );
    c.bench_function("rmcp/call_tool_roundtrip", |b| {
        b.to_async(&rt).iter(|| async {
            let result = rmcp_client
                .call_tool(
                    CallToolRequestParams::new("add").with_arguments(object!({ "a": 2, "b": 3 })),
                )
                .await
                .expect("call_tool");
            black_box(result);
        });
    });
}

criterion_group!(benches, bench_call_tool);
criterion_main!(benches);
