//! The same one-tool server in both SDKs, and each SDK's client connected to
//! its own server over an in-process duplex: what the comparison benchmark
//! and the performance parity test measure.

use std::hint::black_box;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ContentBlock, Implementation, ProtocolVersion,
    ServerCapabilities, ServerConfig,
};
use rmcp::service::{ClientLifecycleMode, ClientServiceExt, RoleClient, RunningService};
use rmcp::{
    ErrorData as RmcpError, ServerHandler, ServiceExt, object, schemars, tool, tool_handler,
    tool_router,
};
use tokio::io::{BufReader, split};
use turbomcp::client::{Client, ClientBuilder, ConnectMode};
use turbomcp::prelude::*;
use turbomcp::{LegacySessionAdapter, SerdeJsonCodec, serve};
use turbomcp_service::io::LineTransport;

/// The revision a pair speaks.
#[derive(Clone, Copy, Debug)]
pub enum Era {
    /// `2025-11-25`: `initialize`, a stateful session.
    Legacy,
    /// `2026-07-28`: `server/discover`, stateless.
    Modern,
}

#[derive(Clone)]
struct RmcpAdder {
    #[allow(dead_code)]
    tool_router: ToolRouter<RmcpAdder>,
    protocol_version: ProtocolVersion,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct AddArgs {
    a: i64,
    b: i64,
}

#[tool_router]
impl RmcpAdder {
    fn new(protocol_version: ProtocolVersion) -> Self {
        Self {
            tool_router: Self::tool_router(),
            protocol_version,
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
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_protocol_version(self.protocol_version.clone())
    }
}

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

/// A turbomcp client connected to a turbomcp server on `era`.
pub async fn turbomcp(era: Era) -> Client {
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let (s_rd, s_wr) = split(server_io);
    let transport = LineTransport::new(BufReader::new(s_rd), s_wr, SerdeJsonCodec);
    tokio::spawn(serve(
        transport,
        LegacySessionAdapter::new(TurboAdder.into_server().build()),
    ));
    let (c_rd, c_wr) = split(client_io);
    ClientBuilder::new("bench-client", "1.0.0")
        .with_connect_mode(match era {
            Era::Legacy => ConnectMode::Legacy,
            Era::Modern => ConnectMode::Modern,
        })
        .connect(LineTransport::new(
            BufReader::new(c_rd),
            c_wr,
            SerdeJsonCodec,
        ))
        .await
        .expect("turbomcp handshake")
}

/// An rmcp client connected to an rmcp server on `era`.
pub async fn rmcp(era: Era) -> RunningService<RoleClient, ()> {
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let version = match era {
        Era::Legacy => ProtocolVersion::V_2025_11_25,
        Era::Modern => ProtocolVersion::V_2026_07_28,
    };
    let (s_rd, s_wr) = split(server_io);
    let server_version = version.clone();
    tokio::spawn(async move {
        if let Ok(running) = RmcpAdder::new(server_version).serve((s_rd, s_wr)).await {
            let _ = running.waiting().await;
        }
    });
    match era {
        Era::Legacy => ().serve(client_io).await.expect("rmcp handshake"),
        Era::Modern => ()
            .serve_with_lifecycle(
                client_io,
                ClientLifecycleMode::Discover {
                    preferred_versions: vec![version],
                },
            )
            .await
            .expect("rmcp discovery"),
    }
}

/// One `add(2, 3)` through turbomcp, checked.
pub async fn turbomcp_call(client: &Client) {
    let mut args = serde_json::Map::new();
    args.insert("a".into(), serde_json::json!(2));
    args.insert("b".into(), serde_json::json!(3));
    let result = client.call_tool("add", args).await.expect("call_tool");
    debug_assert_eq!(result.text_content().as_deref(), Some("5"));
    black_box(result);
}

/// One `add(2, 3)` through rmcp, checked.
pub async fn rmcp_call(client: &RunningService<RoleClient, ()>) {
    let result = client
        .call_tool(CallToolRequestParams::new("add").with_arguments(object!({ "a": 2, "b": 3 })))
        .await
        .expect("call_tool");
    debug_assert_ne!(result.is_error, Some(true));
    black_box(result);
}
