# Documentation and critical-test audit, 2026-09-09

> Subsequent update: legacy recovery and fixture gaps are addressed in the
> [conformance correction record](../crates/turbomcp-conformance/fixtures/README.md).
> Counts and warnings below describe the earlier audit snapshot.

Applied to the alpha.4 candidate working tree, following initial release
preparation. The earlier release verification document is a dated snapshot.

## Corrected documentation and tooling

Updated contributor commands, versioning boundaries, release procedures, macro
schema lifecycle, three-revision protocol documentation, migration lookup costs,
HTTP proxy policy, and the actual benchmark targets. Historical audits are now
marked as historical; they do not establish current v4 behavior. The benchmark
guide no longer promises nonexistent suites, thresholds, or CI performance gates.

[Documentation index](README.md) and [critical coverage guide](TESTING.md) define
supported products and evidence. A local Markdown file-link check now runs in CI.
It covers maintained docs, not archived findings, remote URLs, or anchors.

The publication shell entry point delegates to the justfile's dependency graph
and stops on failures. Its previous hand-maintained list, no-verify publishing,
and continue-after-failure behavior were removed. Syntax, invalid arguments,
and default dry-run checks were exercised; publication was not performed.

There is no v4 standalone MCP proxy or CLI product. Trusted reverse-proxy HTTP
support and the private code generator CLI exist. The future proxy/CLI needs
its own implementation and upstream lifecycle/auth/notification tests; neither
is represented as supported by these documentation changes.

## Defects found through critical-path review

- Trusted proxy parsing skipped malformed hops. It now stops at that boundary
  and uses the socket peer rather than accepting an address farther left.
  Regression cases include malformed/empty hops, IPv6, untrusted-left entries,
  trusted chains, untrusted socket peers, and missing socket information.
- Concurrent failed OAuth refreshes serialized but did not coalesce failure:
  24 waiting callers caused 24 rejected refresh requests. The coordinator now
  shares the failure until fresh authorization. A regression failed before the
  fix and passes afterward, including recovery through a new challenge.
- Macro-generated tool lookup reconstructed schemas per call. Immutable metadata
  now initializes once per generated site; callers receive independent copies.
  Custom dynamic providers retain their own lookup semantics. Schema derivation
  for macro tools is documented as deterministic.

New tests cover concurrent first authorization, stale challenges, concurrent
refresh rotation, failed refresh/recovery, malformed JSON-RPC variants with null
response fidelity, and lookup/list metadata agreement and mutation isolation.
Existing schema, header, visibility, lifecycle, session/task identity, and
independent conformance tests remain required for the optimization.

## Performance result and limits

The official comparator is [rmcp](https://github.com/modelcontextprotocol/rust-sdk),
pinned by the interoperability lockfile. The benchmark is a successful legacy
2025-11-25 tools/call over a Tokio duplex pipe, each SDK using its own client
and server. No handshake is timed. Criterion used its defaults: 100 samples,
3-second warmup, 5-second measurement. Toolchain: rustc 1.98.0,
aarch64-apple-darwin. Hardware model and power/thermal state were not recorded;
the result is a local microbenchmark, not a portable throughput guarantee.

| Measurement | Before | After |
| --- | --- | --- |
| TurboMCP roundtrip point estimate | 27.120 µs | 25.111 µs |
| TurboMCP interval | 27.083–27.154 µs | 25.043–25.189 µs |
| rmcp point estimate in corresponding run | 25.714 µs | 24.949 µs |

Criterion reports a 6.8243% estimated reduction for TurboMCP, with a reduction
interval of 6.5331–7.0981%. The rmcp comparison did not detect a significant
change. TurboMCP was still slightly slower in the after run. The fixtures use
i64 arguments in TurboMCP and i32 in rmcp for the same small integer inputs;
the final follow-up below harmonizes them. No validation or authorization was disabled.

Commands, run sequentially for the before/after benchmark pair:

```sh
cargo bench --manifest-path crates/turbomcp-interop/Cargo.toml --bench sdk_comparison --locked --offline -- --save-baseline before_metadata_cache
# Apply the macro metadata cache change.
cargo bench --manifest-path crates/turbomcp-interop/Cargo.toml --bench sdk_comparison --locked --offline -- --baseline before_metadata_cache
```

Raw outputs: [before](evidence/metadata-cache-before.txt),
[after](evidence/metadata-cache-after.txt). Some regression compilation overlapped
preparation of the after run, so controlled repeated idle-machine runs remain
necessary. HTTP/load/allocation/tail-latency leadership has not been established.

Conformance retains its explicit two header skips and two legacy SSE warnings.
The goal is strict compliance for supported behavior; these measurements and
finite tests do not certify complete compliance or superiority over another SDK.

## Verification

The final workspace run passed 715 tests with zero failures or ignored tests.
All-feature/all-target Clippy, formatting, nightly docs.rs documentation, Rust
1.88, renamed-dependency checks, four rmcp interop tests, and strict conformance
passed. The public OAuth tests include three concurrency/failure scenarios.
Local logs use `/tmp/turbomcp-doc-audit-*.log`; conformance output is at
`/tmp/turbomcp-alpha4-conformance.log`. Formatting and local file links are clean.
No release or commit was created.

## Follow-up with matched argument types

After the correctness builds finished, both SDK fixtures used i64 arguments.
The same default Criterion settings produced TurboMCP 25.597 µs
(interval 25.534–25.661) and rmcp 25.539 µs (25.450–25.632). These intervals
overlap: the measured sequential duplex workload is effectively at parity,
not evidence of TurboMCP being faster. Raw [matched-argument output](evidence/matched-arguments.txt)
is retained. Command:

```sh
cargo bench --manifest-path crates/turbomcp-interop/Cargo.toml --bench sdk_comparison --locked --offline -- --save-baseline alpha4_matched_args
```
