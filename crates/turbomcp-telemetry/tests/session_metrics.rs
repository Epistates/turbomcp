//! Both ends measure a session: `mcp.client.session.duration` when the
//! client's connection closes, `mcp.server.session.duration` when the
//! server's stateful session ends.
#![cfg(feature = "client")]

use std::sync::Arc;

use opentelemetry::KeyValue;
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData, ResourceMetrics};
use opentelemetry_sdk::metrics::{InMemoryMetricExporter, PeriodicReader, SdkMeterProvider};
use tokio::io::{BufReader, split};
use turbomcp_client::{ClientBuilder, ConnectMode};
use turbomcp_core::codec::DefaultCodec;
use turbomcp_core::{Implementation, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    IntoServerBuilder, ListToolsContext, McpServerCore, MethodRouter, WithTools,
};
use turbomcp_service::io::LineTransport;
use turbomcp_telemetry::{ClientTelemetry, MetricsLayer};

#[derive(Clone)]
struct Empty;

impl McpServerCore for Empty {
    fn server_info(&self) -> Implementation {
        Implementation::new("empty", "1.0.0")
    }

    fn register(router: MethodRouter<Self>) -> MethodRouter<Self> {
        router.with_tools()
    }
}

impl WithTools for Empty {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![]))
    }

    async fn call_tool(
        &self,
        _ctx: &turbomcp_server::CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        Ok(neutral::CallToolResult::text(""))
    }
}

/// The attributes of every data point of histogram `name`.
fn points(finished: &[ResourceMetrics], name: &str) -> Vec<Vec<KeyValue>> {
    let Some(snapshot) = finished.last() else {
        return Vec::new();
    };
    snapshot
        .scope_metrics()
        .flat_map(|scope| scope.metrics())
        .filter(|m| m.name() == name)
        .flat_map(|m| match m.data() {
            AggregatedMetrics::F64(MetricData::Histogram(h)) => h
                .data_points()
                .map(|dp| dp.attributes().cloned().collect())
                .collect(),
            _ => Vec::new(),
        })
        .collect()
}

fn has(attrs: &[KeyValue], key: &str, value: &str) -> bool {
    attrs
        .iter()
        .any(|kv| kv.key.as_str() == key && kv.value.as_str() == value)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn both_ends_measure_a_session() {
    let exporter = InMemoryMetricExporter::default();
    let provider = SdkMeterProvider::builder()
        .with_reader(PeriodicReader::builder(exporter.clone()).build())
        .build();
    opentelemetry::global::set_meter_provider(provider.clone());

    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let (s_rd, s_wr) = split(server_io);
    let metrics = MetricsLayer::new();
    let serving = tokio::spawn(
        Empty
            .into_server()
            .observe_sessions(Arc::new(metrics.clone()))
            .layer(metrics)
            .serve(LineTransport::new(
                BufReader::new(s_rd),
                s_wr,
                DefaultCodec::default(),
            )),
    );
    let (c_rd, c_wr) = split(client_io);
    let client = ClientBuilder::new("agent", "1.0.0")
        .with_connect_mode(ConnectMode::Legacy)
        .with_observer(Arc::new(ClientTelemetry::new()))
        .connect(LineTransport::new(
            BufReader::new(c_rd),
            c_wr,
            DefaultCodec::default(),
        ))
        .await
        .expect("connect");
    client.list_tools(None).await.expect("list");
    client.close().await;
    drop(client);
    serving.await.unwrap().unwrap();

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        provider.force_flush().unwrap();
        let finished = exporter.get_finished_metrics().unwrap();
        let server = points(&finished, "mcp.server.session.duration");
        let client = points(&finished, "mcp.client.session.duration");
        if !server.is_empty() && !client.is_empty() {
            assert!(
                has(&server[0], "mcp.protocol.version", "2025-11-25"),
                "{server:?}"
            );
            assert!(
                has(&client[0], "mcp.protocol.version", "2025-11-25"),
                "{client:?}"
            );
            assert!(has(&client[0], "network.transport", "pipe"), "{client:?}");
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "server {server:?}, client {client:?}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}
