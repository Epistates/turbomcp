//! Throughput and tail latency under concurrency, turbomcp and rmcp side by
//! side: each SDK's own client and server, the same one-tool server, over an
//! in-process pipe and over Streamable HTTP on loopback, on both revisions.
//!
//! Off by default; it returns at once unless `TURBOMCP_LOAD` is set:
//!
//! ```sh
//! cd crates/turbomcp-interop
//! TURBOMCP_LOAD=1 cargo test --release --test load -- --nocapture
//! ```
//!
//! For each cell, `C` tasks share one client and call `add` in a loop for
//! `TURBOMCP_LOAD_SECS` (default 10) after a one-second warm-up. The table
//! reports calls per second and the p50 and p99 latency of a call. The
//! concurrency levels are `TURBOMCP_LOAD_CONCURRENCY` (default `1,16,64`);
//! `TURBOMCP_LOAD_SDKS` and `TURBOMCP_LOAD_TRANSPORTS` narrow the run
//! (defaults `turbomcp,rmcp` and `pipe,http`).
//! Rows also go to `TURBOMCP_LOAD_OUT` (default `target/load/<unix time>.csv`).
//!
//! rmcp is left out over HTTP unless `TURBOMCP_LOAD_RMCP_HTTP` is set: its
//! Streamable HTTP client (3.11) opens a TCP connection for every call, and a
//! tight loop runs macOS out of local ports in under three seconds (16k
//! sockets in `TIME_WAIT`), which ends the run.
//!
//! Both SDKs run in this one process, one cell at a time, so memory isn't
//! reported: it would mix the two.

#[path = "../fixtures/adders.rs"]
mod adders;

use std::io::Write as _;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use adders::Era;

/// One client of either SDK, shared by every task in a cell.
enum Sdk {
    Turbo(Arc<turbomcp::client::Client>),
    Rmcp(Arc<rmcp::service::RunningService<rmcp::service::RoleClient, ()>>),
}

impl Sdk {
    async fn call(&self) {
        match self {
            Self::Turbo(client) => adders::turbomcp_call(client).await,
            Self::Rmcp(client) => adders::rmcp_call(client).await,
        }
    }
}

fn list(var: &str, default: &str) -> Vec<String> {
    std::env::var(var)
        .unwrap_or_else(|_| default.into())
        .split(',')
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

fn percentile(sorted: &[u32], p: f64) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    let i = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[i]
}

/// Run `concurrency` tasks against `sdk` for `secs`; calls per second, p50
/// and p99 in microseconds.
async fn cell(sdk: Arc<Sdk>, concurrency: usize, secs: u64) -> (f64, u32, u32) {
    let warm = Instant::now() + Duration::from_secs(1);
    while Instant::now() < warm {
        sdk.call().await;
    }
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut tasks = Vec::with_capacity(concurrency);
    for _ in 0..concurrency {
        let sdk = sdk.clone();
        tasks.push(tokio::spawn(async move {
            let mut latencies = Vec::new();
            while Instant::now() < deadline {
                let start = Instant::now();
                sdk.call().await;
                latencies.push(u32::try_from(start.elapsed().as_micros()).unwrap_or(u32::MAX));
            }
            latencies
        }));
    }
    let mut all = Vec::new();
    for task in tasks {
        all.extend(task.await.expect("load task"));
    }
    all.sort_unstable();
    let rate = all.len() as f64 / secs as f64;
    (rate, percentile(&all, 0.50), percentile(&all, 0.99))
}

#[tokio::test(flavor = "multi_thread")]
async fn load() {
    if std::env::var_os("TURBOMCP_LOAD").is_none() {
        eprintln!("load: skipped (set TURBOMCP_LOAD=1 to run it)");
        return;
    }
    let secs: u64 = std::env::var("TURBOMCP_LOAD_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    let levels: Vec<usize> = std::env::var("TURBOMCP_LOAD_CONCURRENCY")
        .unwrap_or_else(|_| "1,16,64".into())
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let out_path = std::env::var("TURBOMCP_LOAD_OUT")
        .unwrap_or_else(|_| format!("../../target/load/{started}.csv"));
    if let Some(dir) = std::path::Path::new(&out_path).parent() {
        std::fs::create_dir_all(dir).expect("output directory");
    }
    let mut out = std::fs::File::create(&out_path).expect("output file");
    writeln!(
        out,
        "transport,revision,sdk,concurrency,calls_per_sec,p50_us,p99_us"
    )
    .expect("write");

    println!("| transport | revision | SDK | concurrency | calls/s | p50 µs | p99 µs |");
    println!("|---|---|---|---:|---:|---:|---:|");
    let sdks = list("TURBOMCP_LOAD_SDKS", "turbomcp,rmcp");
    for transport in list("TURBOMCP_LOAD_TRANSPORTS", "pipe,http") {
        let transport = transport.as_str();
        for (era, revision) in [(Era::Legacy, "2025-11-25"), (Era::Modern, "2026-07-28")] {
            let mut clients = Vec::new();
            for name in &sdks {
                if name == "rmcp"
                    && transport == "http"
                    && std::env::var_os("TURBOMCP_LOAD_RMCP_HTTP").is_none()
                {
                    continue;
                }
                let sdk = match (name.as_str(), transport) {
                    ("turbomcp", "pipe") => Sdk::Turbo(Arc::new(adders::turbomcp(era).await)),
                    ("turbomcp", _) => Sdk::Turbo(Arc::new(adders::turbomcp_http(era).await)),
                    (_, "pipe") => Sdk::Rmcp(Arc::new(adders::rmcp(era).await)),
                    _ => Sdk::Rmcp(Arc::new(adders::rmcp_http(era).await)),
                };
                clients.push((name.clone(), Arc::new(sdk)));
            }
            for &concurrency in &levels {
                for (name, sdk) in &clients {
                    let (rate, p50, p99) = cell(sdk.clone(), concurrency, secs).await;
                    println!(
                        "| {transport} | {revision} | {name} | {concurrency} | {rate:.0} | {p50} | {p99} |"
                    );
                    writeln!(
                        out,
                        "{transport},{revision},{name},{concurrency},{rate:.0},{p50},{p99}"
                    )
                    .expect("write");
                }
            }
        }
    }
    eprintln!("load: rows in {out_path}");
}
