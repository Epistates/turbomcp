//! Performance parity with the official Rust SDK, as a gate: a turbomcp
//! client and server must complete a `tools/call` round trip within a factor
//! of rmcp's, on both revisions, measured in the same process.
//!
//! Opt-in (`TURBOMCP_PERF=1`), since timing on a shared machine is never
//! exact. The two SDKs run interleaved, batch by batch, so drift in the
//! machine's speed lands on both, and the comparison is of medians. The
//! factor defaults to 1.25 (`TURBOMCP_PERF_FACTOR` overrides it). The
//! numbers are printed either way; `benches/sdk_comparison.rs` measures the
//! same thing in detail.

#[path = "../fixtures/adders.rs"]
mod adders;

use std::time::{Duration, Instant};

use adders::Era;

const WARMUP: usize = 500;
const BATCHES: usize = 25;
const PER_BATCH: usize = 200;

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn turbomcp_keeps_pace_with_rmcp() {
    if std::env::var_os("TURBOMCP_PERF").is_none() {
        eprintln!("skipping: set TURBOMCP_PERF=1 to measure");
        return;
    }
    let factor: f64 = std::env::var("TURBOMCP_PERF_FACTOR")
        .ok()
        .and_then(|f| f.parse().ok())
        .unwrap_or(1.25);
    for era in [Era::Legacy, Era::Modern] {
        let turbo = adders::turbomcp(era).await;
        let rmcp = adders::rmcp(era).await;
        for _ in 0..WARMUP {
            adders::turbomcp_call(&turbo).await;
            adders::rmcp_call(&rmcp).await;
        }
        let (mut ours, mut theirs) = (Vec::new(), Vec::new());
        for _ in 0..BATCHES {
            let start = Instant::now();
            for _ in 0..PER_BATCH {
                adders::turbomcp_call(&turbo).await;
            }
            ours.push(start.elapsed() / PER_BATCH as u32);
            let start = Instant::now();
            for _ in 0..PER_BATCH {
                adders::rmcp_call(&rmcp).await;
            }
            theirs.push(start.elapsed() / PER_BATCH as u32);
        }
        let (ours, theirs) = (median(ours), median(theirs));
        let ratio = ours.as_secs_f64() / theirs.as_secs_f64();
        println!("{era:?}: turbomcp {ours:?}, rmcp {theirs:?} per call (ratio {ratio:.2})");
        assert!(
            ratio <= factor,
            "{era:?}: turbomcp {ours:?} vs rmcp {theirs:?} per call, {ratio:.2}× (limit {factor}×)"
        );
    }
}
