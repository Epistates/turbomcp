//! Server-initiated SSE polling end to end: the endpoint cuts each response
//! stream's connection short, and the real client polls it back with
//! `Last-Event-ID` until the response arrives (`2025-11-25`, SEP-1699).
#![cfg(all(feature = "http", feature = "client"))]

use std::sync::Arc;
use std::time::Duration;

use serde_json::Map;
use turbomcp::client::{ClientBuilder, ConnectMode, connect_http};
use turbomcp::http::{Http, HttpConfig, InMemoryEventStore, SsePolling};
use turbomcp::prelude::*;

#[derive(Clone)]
struct Slow;

#[server(name = "slow", version = "1.0.0")]
impl Slow {
    /// Reports progress over half a second, then answers.
    #[tool]
    async fn crunch(&self, ctx: &CallToolContext) -> String {
        for step in 1..=5 {
            ctx.progress.report(f64::from(step), Some(5.0), None).await;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        "crunched".into()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_polled_call_completes() {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let shutdown = turbomcp::CancellationToken::new();
    tokio::spawn(
        Slow.into_server().serve(
            Http::listener(listener).config(
                HttpConfig::new()
                    .with_event_store(Arc::new(InMemoryEventStore::new()))
                    .with_sse_polling(SsePolling::new(
                        Duration::from_millis(120),
                        Duration::from_millis(20),
                    ))
                    // Upgrade to a stream at once, so polling has one to cut.
                    .sse_upgrade_after(Duration::from_millis(10))
                    .with_shutdown(shutdown.clone()),
            ),
        ),
    );

    let client = connect_http(
        ClientBuilder::new("c", "1.0.0").with_connect_mode(ConnectMode::Legacy),
        &url,
    )
    .await
    .expect("connect");
    let result = client
        .call_tool("crunch", Map::new())
        .await
        .expect("the call completes across polls");
    assert_eq!(result.text_content().as_deref(), Some("crunched"));
    shutdown.cancel();
}
