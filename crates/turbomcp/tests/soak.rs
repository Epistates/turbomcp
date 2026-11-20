//! A soak run: one Streamable HTTP server under mixed traffic for a long
//! time, watched for growth that never levels off.
//!
//! Off by default; it returns at once unless `TURBOMCP_SOAK_SECS` is set:
//!
//! ```sh
//! TURBOMCP_SOAK_SECS=14400 cargo test --release -p turbomcp --all-features \
//!   --test soak -- --nocapture
//! ```
//!
//! The traffic, from concurrent tasks for the whole run:
//! - steady `tools/call`s, stateless (`2026-07-28`);
//! - session churn: a stateful client connects, calls once, and closes, which
//!   sends the session's `DELETE`;
//! - cancellation storms: a call to a slow tool, cancelled a few milliseconds
//!   after it is sent;
//! - `subscriptions/listen` streams opened and dropped.
//!
//! Churn, cancels and listens each run at `TURBOMCP_SOAK_RATE` per second per
//! task (default 10). Every one of them ends a TCP connection, and an
//! unpaced loop runs the OS out of local ports within seconds (closed
//! connections sit in `TIME_WAIT`), which measures the OS, not the server.
//!
//! Every `TURBOMCP_SOAK_SAMPLE_SECS` (default 10) it records the process's
//! resident memory and open file descriptors, the calls and errors since the
//! last sample, and that window's p50 and p99 latency, as one CSV row in
//! `TURBOMCP_SOAK_OUT` (default `target/soak/<unix time>.csv`).
//!
//! It fails on any error a well-behaved server shouldn't produce, and on
//! growth that doesn't level off: after the first quarter of the run (the
//! warm-up: allocator pools, caches, the session table), the median memory
//! of the last quarter may exceed the second quarter's by at most 20% or
//! 32 MiB, whichever is larger, and open descriptors by at most 64.
#![cfg(all(feature = "client", feature = "http"))]

use std::io::Write as _;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Map, json};
use turbomcp::CancellationToken;
use turbomcp::client::{CallOptions, Client, ClientBuilder, ConnectMode, connect_http};
use turbomcp::http::{Http, HttpConfig};
use turbomcp::prelude::*;

#[derive(Clone)]
struct Soak;

#[server(name = "soak", version = "1.0.0")]
impl Soak {
    /// Echo a word back.
    #[tool]
    async fn echo(&self, word: String) -> String {
        word
    }

    /// Wait `ms` milliseconds, or until the call is cancelled.
    #[tool]
    async fn slow(&self, ctx: &CallToolContext, ms: u64) -> String {
        tokio::select! {
            () = ctx.base.cancellation.cancelled() => "cancelled".into(),
            () = tokio::time::sleep(Duration::from_millis(ms)) => "done".into(),
        }
    }
}

/// What every task adds to, read and reset by the sampler.
#[derive(Default)]
struct Counters {
    calls: AtomicU64,
    errors: AtomicU64,
    sessions: AtomicU64,
    cancels: AtomicU64,
    listens: AtomicU64,
    /// Steady-call latencies in this window, in microseconds.
    latencies: std::sync::Mutex<Vec<u32>>,
}

impl Counters {
    fn error(&self, what: &str, e: &dyn std::fmt::Display) {
        self.errors.fetch_add(1, Ordering::Relaxed);
        eprintln!("soak: {what}: {e}");
    }
}

/// How long each paced loop waits between iterations.
fn pace() -> Duration {
    Duration::from_millis(1_000 / env_u64("TURBOMCP_SOAK_RATE", 10).max(1))
}

/// After an error, so a failing loop doesn't spin.
const BACKOFF: Duration = Duration::from_millis(250);

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn args(word: &str) -> Map<String, serde_json::Value> {
    json!({ "word": word })
        .as_object()
        .cloned()
        .unwrap_or_default()
}

/// This process's resident memory in KiB, from `ps` (macOS and Linux alike).
fn rss_kib() -> u64 {
    std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// This process's open file descriptors.
fn open_fds() -> usize {
    let dir = if cfg!(target_os = "linux") {
        "/proc/self/fd"
    } else {
        "/dev/fd"
    };
    std::fs::read_dir(dir).map(Iterator::count).unwrap_or(0)
}

fn percentile(sorted: &[u32], p: f64) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "an index into a short vector"
    )]
    let i = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[i]
}

fn median(values: &mut [u64]) -> u64 {
    values.sort_unstable();
    values.get(values.len() / 2).copied().unwrap_or(0)
}

async fn client(url: &str, mode: ConnectMode, name: &str) -> Option<Client> {
    connect_http(
        ClientBuilder::new(name, "1.0.0").with_connect_mode(mode),
        url,
    )
    .await
    .ok()
}

/// Steady stateless calls, timed.
async fn steady(url: String, deadline: Instant, counters: Arc<Counters>, id: usize) {
    let Some(client) = client(&url, ConnectMode::Stateless, "steady").await else {
        counters.error("steady connect", &"failed");
        return;
    };
    let word = format!("w{id}");
    while Instant::now() < deadline {
        let start = Instant::now();
        match client.call_tool("echo", args(&word)).await {
            Ok(result) if !result.is_error => {
                counters.calls.fetch_add(1, Ordering::Relaxed);
                let micros = u32::try_from(start.elapsed().as_micros()).unwrap_or(u32::MAX);
                counters
                    .latencies
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(micros);
            }
            Ok(result) => counters.error("steady call", &format!("{result:?}")),
            Err(e) => {
                counters.error("steady call", &e);
                tokio::time::sleep(BACKOFF).await;
            }
        }
    }
    client.close().await;
}

/// Stateful sessions, opened, used once and closed (`DELETE`).
async fn churn(url: String, deadline: Instant, counters: Arc<Counters>) {
    let mut tick = tokio::time::interval(pace());
    while Instant::now() < deadline {
        tick.tick().await;
        let Some(client) = client(&url, ConnectMode::Stateful, "churn").await else {
            counters.error("churn connect", &"failed");
            tokio::time::sleep(BACKOFF).await;
            continue;
        };
        match client.call_tool("echo", args("churn")).await {
            Ok(_) => {
                counters.sessions.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => counters.error("churn call", &e),
        }
        client.close().await;
    }
}

/// Calls cancelled a few milliseconds after they are sent.
async fn cancel_storm(url: String, deadline: Instant, counters: Arc<Counters>) {
    let Some(client) = client(&url, ConnectMode::Stateful, "cancel").await else {
        counters.error("cancel connect", &"failed");
        return;
    };
    let mut slow = Map::new();
    slow.insert("ms".into(), json!(5_000));
    let mut tick = tokio::time::interval(pace());
    while Instant::now() < deadline {
        tick.tick().await;
        let token = CancellationToken::new();
        let options = CallOptions::new().with_cancellation(token.clone());
        let call = client.call_tool_with("slow", slow.clone(), &options);
        let cancel = async {
            tokio::time::sleep(Duration::from_millis(5)).await;
            token.cancel();
        };
        let (result, ()) = tokio::join!(call, cancel);
        match result {
            Err(turbomcp::client::ClientError::Cancelled) => {
                counters.cancels.fetch_add(1, Ordering::Relaxed);
            }
            Ok(_) => counters.error("cancel", &"finished despite the cancel"),
            Err(e) => {
                counters.error("cancel", &e);
                tokio::time::sleep(BACKOFF).await;
            }
        }
    }
    client.close().await;
}

/// `subscriptions/listen` streams, opened and dropped.
async fn listens(url: String, deadline: Instant, counters: Arc<Counters>) {
    let Some(client) = client(&url, ConnectMode::Stateless, "listen").await else {
        counters.error("listen connect", &"failed");
        return;
    };
    let mut tick = tokio::time::interval(pace());
    while Instant::now() < deadline {
        tick.tick().await;
        match client
            .listen(neutral::SubscriptionFilter::all_list_changed())
            .await
        {
            Ok(subscription) => {
                counters.listens.fetch_add(1, Ordering::Relaxed);
                tokio::time::sleep(Duration::from_millis(200)).await;
                drop(subscription);
            }
            Err(e) => {
                counters.error("listen", &e);
                tokio::time::sleep(BACKOFF).await;
            }
        }
    }
    client.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn soak() {
    let secs = env_u64("TURBOMCP_SOAK_SECS", 0);
    if secs == 0 {
        eprintln!("soak: skipped (set TURBOMCP_SOAK_SECS to run it)");
        return;
    }
    let sample = Duration::from_secs(env_u64("TURBOMCP_SOAK_SAMPLE_SECS", 10));
    let steady_clients = env_u64("TURBOMCP_SOAK_CLIENTS", 16);
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let out_path = std::env::var("TURBOMCP_SOAK_OUT")
        .unwrap_or_else(|_| format!("../../target/soak/{started}.csv"));
    if let Some(dir) = std::path::Path::new(&out_path).parent() {
        std::fs::create_dir_all(dir).expect("output directory");
    }
    let mut out = std::fs::File::create(&out_path).expect("output file");
    writeln!(
        out,
        "elapsed_s,rss_kib,open_fds,calls,errors,sessions,cancels,listens,p50_us,p99_us"
    )
    .expect("write");

    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind");
    let addr: SocketAddr = listener.local_addr().expect("address");
    let shutdown = CancellationToken::new();
    let server = tokio::spawn(Soak.into_server().serve(
        Http::listener(listener).with_config(HttpConfig::new().with_shutdown(shutdown.clone())),
    ));
    let url = format!("http://{addr}/mcp");

    let begin = Instant::now();
    let deadline = begin + Duration::from_secs(secs);
    let counters = Arc::new(Counters::default());
    let mut tasks = Vec::new();
    for id in 0..usize::try_from(steady_clients).unwrap_or(16) {
        tasks.push(tokio::spawn(steady(
            url.clone(),
            deadline,
            counters.clone(),
            id,
        )));
    }
    for _ in 0..2 {
        tasks.push(tokio::spawn(churn(url.clone(), deadline, counters.clone())));
        tasks.push(tokio::spawn(cancel_storm(
            url.clone(),
            deadline,
            counters.clone(),
        )));
    }
    tasks.push(tokio::spawn(listens(
        url.clone(),
        deadline,
        counters.clone(),
    )));

    let mut rows: Vec<(u64, u64, usize)> = Vec::new();
    let mut ticker = tokio::time::interval(sample);
    ticker.tick().await;
    while Instant::now() < deadline {
        ticker.tick().await;
        let mut window = std::mem::take(
            &mut *counters
                .latencies
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        window.sort_unstable();
        let elapsed = begin.elapsed().as_secs();
        let (rss, fds) = (rss_kib(), open_fds());
        writeln!(
            out,
            "{elapsed},{rss},{fds},{},{},{},{},{},{},{}",
            counters.calls.swap(0, Ordering::Relaxed),
            counters.errors.load(Ordering::Relaxed),
            counters.sessions.swap(0, Ordering::Relaxed),
            counters.cancels.swap(0, Ordering::Relaxed),
            counters.listens.swap(0, Ordering::Relaxed),
            percentile(&window, 0.50),
            percentile(&window, 0.99),
        )
        .expect("write");
        out.flush().expect("flush");
        rows.push((elapsed, rss, fds));
    }
    for task in tasks {
        let _ = task.await;
    }
    shutdown.cancel();
    let _ = server.await;

    let errors = counters.errors.load(Ordering::Relaxed);
    eprintln!(
        "soak: {} samples in {out_path}, {errors} errors",
        rows.len()
    );
    assert_eq!(
        errors, 0,
        "errors during the soak; see stderr and {out_path}"
    );

    // Growth that levels off is a warm-up; growth that doesn't is a leak.
    assert!(rows.len() >= 8, "too short a run to judge growth");
    let quarter = rows.len() / 4;
    let mut second: Vec<u64> = rows[quarter..2 * quarter].iter().map(|r| r.1).collect();
    let mut last: Vec<u64> = rows[3 * quarter..].iter().map(|r| r.1).collect();
    let (baseline, end) = (median(&mut second), median(&mut last));
    let allowed = (baseline / 5).max(32 * 1024);
    assert!(
        end <= baseline + allowed,
        "resident memory kept growing: {baseline} KiB in the second quarter, {end} KiB in the last"
    );
    let fds_baseline = rows[quarter].2;
    let fds_end = rows.last().map_or(0, |r| r.2);
    assert!(
        fds_end <= fds_baseline + 64,
        "open descriptors kept growing: {fds_baseline} after warm-up, {fds_end} at the end"
    );
}
