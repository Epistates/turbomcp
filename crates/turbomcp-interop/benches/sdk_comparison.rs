//! Cross-SDK performance comparison: a steady-state `call_tool("add")`
//! round-trip through turbomcp vs the official Rust SDK (`rmcp`), each SDK
//! driving **both ends** (its own client and server) over an in-process
//! `tokio::io::duplex` pipe, on both revisions.
//!
//! The connection and handshake happen once, outside the measured loop; each
//! iteration is one full client→server→client tool call including
//! newline-JSON framing on both sides.
//!
//! This crate is excluded from the workspace (rmcp's dep tree stays out of the
//! main lockfile). Run it directly:
//!   `cd crates/turbomcp-interop && cargo bench --bench sdk_comparison`
//! `tests/perf_parity.rs` turns the same measurement into a gate.
//!
//! `call_tool_roundtrip_http` repeats it over Streamable HTTP on loopback, for
//! turbomcp alone (see `bench_call_tool_http`).

#[path = "../fixtures/adders.rs"]
mod adders;

use adders::Era;
use criterion::{Criterion, criterion_group, criterion_main};

fn bench_call_tool(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    for (era, name) in [(Era::Legacy, "2025-11-25"), (Era::Modern, "2026-07-28")] {
        let mut group = c.benchmark_group(format!("call_tool_roundtrip/{name}"));
        let turbo = rt.block_on(adders::turbomcp(era));
        group.bench_function("turbomcp", |b| {
            b.to_async(&rt).iter(|| adders::turbomcp_call(&turbo));
        });
        let rmcp = rt.block_on(adders::rmcp(era));
        group.bench_function("rmcp", |b| {
            b.to_async(&rt).iter(|| adders::rmcp_call(&rmcp));
        });
        group.finish();
    }
}

/// The same round trip over Streamable HTTP on loopback, turbomcp only, one
/// connection kept alive, handshake outside timing. rmcp is left out: its
/// Streamable HTTP client (3.11) opens a TCP connection for every call, so a
/// benchmark loop runs out of local ports in seconds (see `tests/load.rs`).
fn bench_call_tool_http(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    for (era, name) in [(Era::Legacy, "2025-11-25"), (Era::Modern, "2026-07-28")] {
        let mut group = c.benchmark_group(format!("call_tool_roundtrip_http/{name}"));
        let turbo = rt.block_on(adders::turbomcp_http(era));
        group.bench_function("turbomcp", |b| {
            b.to_async(&rt).iter(|| adders::turbomcp_call(&turbo));
        });
        group.finish();
    }
}

criterion_group!(benches, bench_call_tool, bench_call_tool_http);
criterion_main!(benches);
