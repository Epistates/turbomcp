//! A call its client abandons still counts against the endpoint's limits
//! until its work ends.
//!
//! Under the default, cooperative cancellation, a handler whose request is
//! abandoned (its client hangs up, or the server's `call_timeout` answers
//! it) is told through its token and runs on until it stops. Its request
//! slot has to stay taken for as long: otherwise a client could start a slow
//! call, hang up, and start another, and run more work at once than
//! `with_max_in_flight` allows.

#![cfg(all(feature = "client", feature = "http"))]

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::Map;
use turbomcp::CancellationToken;
use turbomcp::client::{CallOptions, Client, ClientBuilder, ConnectMode, connect_http};
use turbomcp::http::{Http, HttpConfig};
use turbomcp::prelude::*;

/// How many `grind` calls are running now, and the most that ever were.
#[derive(Default)]
struct Gauge {
    running: AtomicUsize,
    most: AtomicUsize,
}

/// Counts one running call for as long as it lives.
struct Running(Arc<Gauge>);

impl Running {
    fn enter(gauge: &Arc<Gauge>) -> Self {
        let now = gauge.running.fetch_add(1, Ordering::SeqCst) + 1;
        gauge.most.fetch_max(now, Ordering::SeqCst);
        Self(Arc::clone(gauge))
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.0.running.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Clone)]
struct Grinder(Arc<Gauge>);

#[server(name = "grinder", version = "1.0.0")]
impl Grinder {
    /// Work for a while without ever checking for cancellation.
    #[tool]
    async fn grind(&self) -> String {
        let _running = Running::enter(&self.0);
        tokio::time::sleep(Duration::from_millis(1500)).await;
        "done".into()
    }
}

const LIMIT: usize = 2;

/// Serve a fresh [`Grinder`] with the in-flight limit (and `call_timeout`,
/// if any), and connect a stateless client to it.
async fn grinder(call_timeout: Option<Duration>) -> (Client, Arc<Gauge>, CancellationToken) {
    let gauge = Arc::new(Gauge::default());
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let shutdown = CancellationToken::new();
    let config = HttpConfig::new()
        .with_max_in_flight(LIMIT)
        .with_shutdown(shutdown.clone());
    let mut server = Grinder(Arc::clone(&gauge)).into_server();
    if let Some(limit) = call_timeout {
        server = server.with_call_timeout(limit);
    }
    tokio::spawn(server.serve(Http::listener(listener).with_config(config)));
    tokio::time::sleep(Duration::from_millis(100)).await;
    let client = connect_http(
        ClientBuilder::new("abandoner", "1.0.0").with_connect_mode(ConnectMode::Stateless),
        &format!("http://{addr}/mcp"),
    )
    .await
    .expect("connect");
    (client, gauge, shutdown)
}

/// Wait for every call started to end, then check none ran past the limit.
async fn assert_within_the_limit(gauge: &Gauge) {
    for _ in 0..100 {
        if gauge.running.load(Ordering::SeqCst) == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let most = gauge.most.load(Ordering::SeqCst);
    assert!(most >= 1, "no call ever ran");
    assert!(
        most <= LIMIT,
        "{most} calls ran at once, past the endpoint's limit of {LIMIT}"
    );
}

/// The client gives up on each call while it runs (hanging up on its
/// stream) and starts another.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_call_its_client_hung_up_on_keeps_its_slot_until_its_work_ends() {
    let (client, gauge, shutdown) = grinder(None).await;
    let give_up = CallOptions::new().with_timeout(Duration::from_millis(100));
    for _ in 0..6 {
        let _ = client.call_tool_with("grind", Map::new(), &give_up).await;
    }
    assert_within_the_limit(&gauge).await;
    shutdown.cancel();
}

/// The server's `call_timeout` answers each call while its handler runs on,
/// and the client starts another.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_call_answered_by_call_timeout_keeps_its_slot_until_its_work_ends() {
    let (client, gauge, shutdown) = grinder(Some(Duration::from_millis(150))).await;
    for _ in 0..6 {
        let _ = client.call_tool("grind", Map::new()).await;
    }
    assert_within_the_limit(&gauge).await;
    shutdown.cancel();
}
