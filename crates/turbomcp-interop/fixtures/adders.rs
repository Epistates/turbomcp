//! The same one-tool server in both SDKs, and each SDK's client connected to
//! its own server, over an in-process duplex or Streamable HTTP on loopback:
//! what the comparison benchmark, the performance parity test and the load
//! test measure. Each of those uses part of it.
#![allow(
    dead_code,
    reason = "each bench and test includes the file and uses part of it"
)]

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
            Era::Legacy => ConnectMode::Stateful,
            Era::Modern => ConnectMode::Stateless,
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

/// A turbomcp server on a loopback Streamable HTTP endpoint, and a turbomcp
/// client connected to it.
pub async fn turbomcp_http(era: Era) -> Client {
    use turbomcp::http::{Http, HttpConfig};
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("address");
    tokio::spawn(
        TurboAdder
            .into_server()
            .serve(Http::listener(listener).with_config(HttpConfig::new())),
    );
    turbomcp::client::connect_http(
        ClientBuilder::new("bench-client", "1.0.0").with_connect_mode(match era {
            Era::Legacy => ConnectMode::Stateful,
            Era::Modern => ConnectMode::Stateless,
        }),
        &format!("http://{addr}/mcp"),
    )
    .await
    .expect("turbomcp http connect")
}

/// An rmcp server on a loopback Streamable HTTP endpoint, and an rmcp client
/// connected to it.
///
/// The server answers with JSON and no sessions, rmcp's fastest HTTP setting.
/// Its default answers each POST with an SSE stream, which its client stops
/// reading once the response arrives, so the connection can't be reused: a
/// tight loop then opens a TCP connection per call and runs out of local
/// ports in seconds.
pub async fn rmcp_http(era: Era) -> RunningService<RoleClient, ()> {
    use rmcp::transport::StreamableHttpClientTransport;
    use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService,
    };
    let version = match era {
        Era::Legacy => ProtocolVersion::V_2025_11_25,
        Era::Modern => ProtocolVersion::V_2026_07_28,
    };
    let server_version = version.clone();
    let service = StreamableHttpService::new(
        move || Ok(RmcpAdder::new(server_version.clone())),
        std::sync::Arc::new(LocalSessionManager::default()),
        {
            let mut config = StreamableHttpServerConfig::default();
            config.json_response = true;
            config.legacy_session_mode = false;
            config
        },
    );
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("address");
    tokio::spawn(async move {
        let app = axum::Router::new().nest_service("/mcp", service);
        let _ = axum::serve(listener, app).await;
    });
    let transport = StreamableHttpClientTransport::from_uri(format!("http://{addr}/mcp"));
    match era {
        Era::Legacy => ().serve(transport).await.expect("rmcp http handshake"),
        Era::Modern => ()
            .serve_with_lifecycle(
                transport,
                ClientLifecycleMode::Discover {
                    preferred_versions: vec![version],
                },
            )
            .await
            .expect("rmcp http discovery"),
    }
}
