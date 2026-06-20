# Perf PR — Transport per-message I/O overhead

**Date:** 2026-06-19
**Branch:** `perf/pr1-transport-io-overhead`

## Summary

Four constant-factor fixes on work performed for **every message** crossing a
transport, each proven with a new criterion benchmark run before and after the
change on identical hardware:

| Crate | Change | Headline result |
|-------|--------|-----------------|
| `turbomcp-http` | Metrics counters: `RwLock<TransportMetrics>` → lock-free `AtomicMetrics` | **+573 % / +482 % throughput** at 4/16 concurrent tasks |
| `turbomcp-server` | SSE subscriber channel payload: `String` → `Arc<str>` | Broadcast fan-out **+220 % (8 sessions) / +299 % (64 sessions)** |
| `turbomcp-server` | `sse_event_bytes`: exact single-allocation buffer sizing | **−90 % (64 B) to −27 % (64 KB)** frame-build time |
| `turbomcp-stdio` | Inbound line → payload: per-line `String` copy → zero-copy `Bytes` slice | **−2.7 % to −20.3 %** parse time, scaling with payload size |

No protocol behavior changes. All 500+ existing tests across the four affected
crates pass; the only test edits are mechanical signature updates plus one new
test pinning the zero-copy trim semantics. One narrowly-scoped trade-off was
measured and accepted (single-subscriber 64 KB sends, +10 %; see
[Trade-offs](#trade-offs-and-regressions-accepted)).

## Methodology

* **Hardware/OS:** AMD Ryzen 5 7500X3D (6C/12T), Windows 10 Pro 19045.
* **Toolchain:** rustc 1.94.1, criterion 0.8.2.
* **Settings:** 100 samples, 1 s warm-up, 3 s measurement, `--noplot`. All
  numbers below are criterion mid-point estimates; criterion reported
  `p = 0.00 < 0.05` for every comparison cited.
* **Protocol:** benchmarks were added in a behavior-neutral scaffolding commit
  (`4068df5`), the baseline was captured with `--save-baseline before` on
  unmodified hot-path code, then the fixes (`b6ad0b5`, `a785e49`) were
  benchmarked with `--baseline before`. Source for the measured paths is
  identical between runs except for the fix itself.

Reproduction:

```sh
git checkout 4068df5   # scaffolding: benches exist, hot paths unmodified
cargo bench -p turbomcp-server --features http,internal-bench --bench sse_throughput -- --save-baseline before
cargo bench -p turbomcp-http --features internal-bench --bench metrics_recording -- --save-baseline before
cargo bench -p turbomcp-stdio --features internal-bench --bench line_parse -- --save-baseline before
git checkout perf/pr1-transport-io-overhead
# re-run the same three commands with: --baseline before
```

## HTTP client metrics: `RwLock` → atomics

`StreamableHttpClientTransport` bumped two counters under a
`tokio::sync::RwLock<TransportMetrics>` **write** lock on every message sent
and received (5 call sites). The fix swaps the struct for the lock-free
`AtomicMetrics` already provided by `turbomcp-transport-traits` and already
used by `turbomcp-stdio`; counters become `fetch_add(…, Relaxed)` and the
`Transport::metrics()` snapshot uses `AtomicMetrics::snapshot()`.

Benchmark: `turbomcp-http/benches/metrics_recording.rs` — N tasks each
recording 1 000 messages on a shared transport, 8-worker runtime.

| Concurrent tasks | Before | After | Time delta |
|---|---|---|---|
| 1 | 23.4 M records/s | 48.2 M records/s | −48.7 % |
| 4 | 9.1 M records/s | 61.6 M records/s | −85.2 % |
| 16 | 9.2 M records/s | 53.6 M records/s | −82.8 % |

The baseline numbers show the problem directly: adding concurrency made the
RwLock path **2.5× slower** than single-threaded (23.4 → 9.1 M/s) because every
record serializes on the write lock and parks/wakes tasks. The atomic path
instead **gains** from concurrency (48 → 62 M/s) and never blocks a message on
metrics bookkeeping.

## SSE subscriber payloads: `String` → `Arc<str>`

`SessionManager` routed every outbound SSE message as `tx.send(message.to_string())`
— a full copy of the payload per send attempt, and per *session* in
`broadcast`. Subscriber channels now carry `Arc<str>`: the payload is allocated
once per routed message and shared by reference everywhere else.

Benchmark: `turbomcp-server/benches/sse_throughput.rs` (`send_to_session`,
`broadcast` groups). Broadcast uses a 4 KB payload across N sessions:

| Broadcast sessions | Before | After | Time delta |
|---|---|---|---|
| 1 | 134 ns | 138 ns | +5.0 % |
| 8 | 943 ns | 298 ns | −68.8 % |
| 64 | 7.79 µs | 1.95 µs | −74.9 % |

`send_to_session` (single subscriber, one allocation either way — the win here
is downstream, where the SSE stream handler shares rather than owns):

| Payload | Before | After | Time delta |
|---|---|---|---|
| 64 B | 293 ns | 143 ns | −50.8 % |
| 4 KB | 206 ns | 178 ns | −14.7 % |
| 64 KB | 1.28 µs | 1.40 µs | **+10.3 %** (see trade-offs) |

## `sse_event_bytes` single-allocation framing

The SSE frame (`id:`/`event:`/`data:` lines) was built into `String::new()`,
paying the grow-by-doubling realloc-and-copy chain on every outbound event. The
buffer is now pre-sized exactly for the dominant single-line JSON-RPC case
(`5 + id.len() + event_overhead + data.len() + 8`, no payload scan); rare
multi-line data regrows from a near-correct base.

| Case | Before | After | Time delta |
|---|---|---|---|
| single line, 64 B | 444 ns | 44 ns | −90.2 % |
| single line, 4 KB | 534 ns | 257 ns | −52.0 % |
| single line, 64 KB | 5.76 µs | 4.24 µs | −27.1 % |
| 64 lines × 64 B | 1.38 µs | 963 ns | −28.6 % |

**Negative result worth recording:** the first implementation sized the buffer
*exactly* by counting `\n` bytes in the payload. That scan regressed 4 KB
frames by +159 % and 64 KB frames by +283 % — a byte-wise count pass costs more
than the reallocations it avoids. The committed version (`a785e49`) sizes
without scanning. If a future change reintroduces exact sizing, it must use a
SIMD count (e.g. `memchr::memchr_iter().count()`) and re-benchmark.

## stdio inbound line: zero-copy payload

`parse_message` built the payload as `Bytes::from(line.to_string())` — a full
copy of every inbound line. The reader task already owns the `String`, so
`parse_message` now takes it by value, converts it to `Bytes` without copying,
and slices to the trimmed range (offset arithmetic on the `trim()` subslice; no
`unsafe`).

Benchmark: `turbomcp-stdio/benches/line_parse.rs` (cost includes the
irreducible `serde_json` validation parse, which dominates at small sizes):

| Line size | Before | After | Time delta |
|---|---|---|---|
| 128 B | 1.08 µs | 1.06 µs | −2.7 % |
| 1 KB | 1.21 µs | 1.16 µs | −3.7 % |
| 8 KB | 2.14 µs | 2.00 µs | −6.6 % |
| 64 KB | 11.4 µs | 9.06 µs | −20.3 % |

The delta grows with payload size exactly as a removed `memcpy + alloc` should.
A new unit test (`test_message_parsing_trims_surrounding_whitespace`) pins that
the sliced payload equals the trimmed line byte-for-byte, and
`test_message_parsing` now asserts payload byte-equality with its input.

## Correctness evidence

* `cargo test -p turbomcp-http` — 7 passed.
* `cargo test -p turbomcp-stdio` — 25 passed (includes the new zero-copy trim
  test and the strengthened payload byte-equality assertion).
* `cargo test -p turbomcp-server --features http` — 155 passed, including the
  SSE wire-format unit tests (`sse_event_bytes_formats_*` byte-exact frame
  assertions) and the bound-port Streamable HTTP integration tests
  (primer event IDs, resumability, single-subscriber routing, sampling
  round-trip over SSE).
* `cargo test -p turbomcp-transport --all-features` — 335 passed.
* `cargo check --workspace --all-features` and `cargo clippy --benches --tests`
  on all touched crates — clean.

The SSE frame format, routing semantics (MCP 2025-11-25 §Multiple Connections:
exactly one stream per message), trim behavior, and metrics snapshot shape are
all covered by pre-existing tests that pass unmodified.

## Trade-offs and regressions accepted

* **`send_to_session` at 64 KB: +10 %** (1.28 → 1.40 µs). `Arc<str>::from`
  copies into a fresh allocation with a refcount header, which for 64 KB
  payloads lands less favorably with the allocator than a bare `String` copy,
  and the receiver pays an atomic refcount drop. Small/medium payloads (the
  JSON-RPC norm) improved 15–51 %, and broadcast improved up to 4×; the 64 KB
  single-subscriber case is dominated by network I/O in practice.
* **`broadcast` to 1 session: +5 %** (134 → 138 ns) — same Arc-header cost,
  nanoseconds in absolute terms, repaid 200×+ at realistic fan-outs.
* **API surface:** To avoid exposing the `Arc<str>` change (and the
  plumbing methods) to normal users, `SessionManager`, `subscribe_session`,
  `send_to_session`, `broadcast`, and `sse_event_bytes` are only `pub`
  (with `#[doc(hidden)]`) when the `internal-bench` Cargo feature is enabled.
  The `[[bench]]` targets declare the feature via `required-features`. Normal
  consumers with `--features http` see only the documented `run*` entry points.
  The performance wins (single `Arc::from` + cheap clones on the send side)
  are unaffected.

## Benchmark inventory added by this PR

| File | Measures |
|------|----------|
| `crates/turbomcp-server/benches/sse_throughput.rs` | SSE framing, session routing, broadcast fan-out |
| `crates/turbomcp-http/benches/metrics_recording.rs` | Metrics record path, 1/4/16-task contention |
| `crates/turbomcp-stdio/benches/line_parse.rs` | Inbound line → `TransportMessage` |
