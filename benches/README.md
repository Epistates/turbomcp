# v4 performance verification

The executable benchmarks are registered in crate manifests:

```sh
cargo bench -p turbomcp --bench dispatch --locked
cargo bench --manifest-path crates/turbomcp-interop/Cargo.toml --bench sdk_comparison --locked
```

`dispatch` measures successful tools/call dispatch plus codec encode/decode.
`sdk_comparison` compares each SDK's own client and server over a Tokio duplex
pipe using legacy `2025-11-25`, with handshake outside timing and successful
result assertions before measurement. The comparison pins rmcp in its separate
lockfile. Root `benches/integration/` files are historical, unregistered v3
experiments; they are not v4 coverage or executable benchmark targets.

Run measurements sequentially on an idle machine, using the same toolchain,
protocol, transport, inputs, features, concurrency, and limits. Record CPU,
memory, OS, exact revision/diff, lockfile, command, raw Criterion output, and
confidence intervals. Use at least the default Criterion warmup/sample settings
for a baseline. See the [metadata-cache measurements](../docs/DOCS-TESTS-AUDIT.md)
for a measured optimization and a matched-argument comparison. Do not compare results across different machines or error paths.

There is no automated 5% performance gate, hardware normalization, allocation
threshold, or throughput certification. Criterion results live in the relevant
target/criterion directory. Short audit measurements did not establish a speed
advantage over rmcp; see [audit evidence](../docs/V4-AUDIT-REMEDIATION.md).

Performance work must retain input/output validation, authorization, cancellation,
and protocol behavior. Before claiming leadership, measure HTTP as well as stdio,
small/large payloads and catalogs, cold/warm calls, concurrent load, p50/p95/p99,
allocations, and memory after repeated connection teardown. Compatibility and
security regression suites must pass after any optimization.
