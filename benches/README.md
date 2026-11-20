# v4 performance verification

The executable benchmarks are registered in crate manifests:

```sh
cargo bench -p turbomcp --bench dispatch --locked
cargo bench --manifest-path crates/turbomcp-interop/Cargo.toml --bench sdk_comparison --locked
```

`dispatch` measures successful tools/call dispatch plus codec encode/decode.
`sdk_comparison` compares each SDK's own client and server over a Tokio duplex
pipe on `2025-11-25` and `2026-07-28`, with handshake outside timing and
successful result assertions before measurement, and repeats the round trip
over Streamable HTTP on loopback for TurboMCP. The comparison pins rmcp in its
separate lockfile. Root `benches/integration/` files are historical,
unregistered v3 experiments; they are not v4 coverage or executable benchmark
targets.

`just perf-sweep` runs both benchmarks and the concurrency load test
(`crates/turbomcp-interop/tests/load.rs`) and records the machine beside the
results. `just soak` runs the four-hour soak (`crates/turbomcp/tests/soak.rs`).
The published results, method and machine are in
[docs/PERFORMANCE.md](../docs/PERFORMANCE.md).

Run measurements sequentially on an idle machine, using the same toolchain,
protocol, transport, inputs, features, concurrency, and limits. Record CPU,
memory, OS, exact revision/diff, lockfile, command, raw Criterion output, and
confidence intervals. Use at least the default Criterion warmup/sample settings
for a baseline. Do not compare results across different machines or error
paths.

There is no automated 5% performance gate, hardware normalization, allocation
threshold, or throughput certification. The one gate is
`crates/turbomcp-interop/tests/perf_parity.rs` (`TURBOMCP_PERF=1`): a
TurboMCP round trip on the pipe must stay within a factor of rmcp's, 1.25 by
default and 1.5 on CI's shared runners. Criterion
results live in the relevant target/criterion directory. The results in
docs/PERFORMANCE.md hold for one machine and a one-tool server; they are not a
general speed claim, and rmcp is faster in some of them.

Performance work must retain input/output validation, authorization, cancellation,
and protocol behavior. Before claiming leadership, measure HTTP as well as stdio,
small/large payloads and catalogs, cold/warm calls, concurrent load, p50/p95/p99,
allocations, and memory after repeated connection teardown. Compatibility and
security regression suites must pass after any optimization.
