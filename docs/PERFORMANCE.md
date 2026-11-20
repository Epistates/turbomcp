# Performance

These are measurements of TurboMCP 4.0.0-beta.1 on one machine, with the
commands that produced them, so anyone can rerun them. They cover a single
dispatch, a tool call's round trip, throughput and tail latency under
concurrency, and a four-hour soak. Where the official Rust SDK (`rmcp`) was
measured too, both numbers are here, including the cases where rmcp is faster.

Nothing here is a general claim. The numbers hold for this machine, this
toolchain, a one-tool server and small messages. Compare results only within one
run on one machine.

## Machine and revision

| | |
|---|---|
| Revision | `57b6b8f1` (4.0.0-beta.1 plus the harnesses) |
| Date | 2026-11-20 |
| CPU | Apple M4 Max, 16 cores |
| Memory | 128 GB |
| OS | macOS (Darwin 27.2.0, arm64) |
| Toolchain | rustc 1.100.0, release profile |
| rmcp | 3.11.0, pinned in `crates/turbomcp-interop/Cargo.lock` |
| tokio | 1.54.1 |

The sweep ran alone in the machine's build queue. Each SDK drives both ends of
the connection, its own client against its own server, with the same one-tool
server: `add(a, b)` on two integers, returning text. The codec is `serde_json`
(the `simd` feature is off).

## How to reproduce

```sh
just perf-sweep   # under 10 minutes here; results in target/perf/<time>/
just soak         # four hours; one CSV row every 10 s in target/soak/
```

`just perf-sweep` records the machine and revision in `machine.txt`, then runs:

- `cargo bench -p turbomcp --bench dispatch` (Criterion);
- `cargo bench --bench sdk_comparison` in `crates/turbomcp-interop` (Criterion);
- the load test, `TURBOMCP_LOAD=1 cargo test --release --test load` in
  `crates/turbomcp-interop`.

Criterion runs with its default warm-up and sample settings. The intervals below
are its 95% confidence intervals.

## One dispatch

The server alone, with no transport: a `tools/call` request handed to the
dispatcher and its response produced (`crates/turbomcp/benches/dispatch.rs`).

| Benchmark | Time |
|---|---|
| `tools/call` | 3.39 µs (3.38-3.40) |
| `tools/call`, authenticated caller | 3.42 µs (3.40-3.44) |
| `tools/call`, wide input schema | 3.89 µs (3.88-3.89) |
| encode one response | 164 ns (164-165) |
| decode one request | 1.16 µs (1.16-1.17) |

## One tool call, round trip

One caller, one call at a time, after the handshake: the client sends
`tools/call`, the server answers, and the client has the result
(`crates/turbomcp-interop/benches/sdk_comparison.rs`). The pipe is an in-process
`tokio::io::duplex` with newline-delimited JSON, the same framing as stdio.
HTTP is Streamable HTTP over loopback.

| Transport | Revision | TurboMCP | rmcp |
|---|---|---|---|
| pipe | `2025-11-25` | 19.17 µs (19.13-19.21) | 24.14 µs (24.08-24.20) |
| pipe | `2026-07-28` | 25.41 µs (25.34-25.48) | 25.86 µs (25.83-25.90) |
| HTTP | `2025-11-25` | 147.67 µs (147.22-148.16) | not measured |
| HTTP | `2026-07-28` | 68.52 µs (68.42-68.61) | not measured |

On the pipe, a TurboMCP call takes about 21% less time than an rmcp call on
`2025-11-25`. On `2026-07-28` the two are within 2% of each other.

Over HTTP, a `2025-11-25` call is stateful (it carries a session) and costs about
twice a stateless `2026-07-28` call. See [open work](#open-work).

## Throughput under concurrency

`C` tasks share one client and call `add` in a loop for 10 seconds, after a
one-second warm-up (`crates/turbomcp-interop/tests/load.rs`). Each cell reports
calls per second and the p50 and p99 latency of a single call. Both SDKs run in
one process, one cell at a time.

### Pipe

| Revision | C | TurboMCP calls/s | p50 / p99 | rmcp calls/s | p50 / p99 |
|---|---|---|---|---|---|
| `2025-11-25` | 1 | 77,687 | 12 / 17 µs | 59,754 | 16 / 20 µs |
| `2025-11-25` | 16 | 177,315 | 89 / 168 µs | 171,028 | 92 / 139 µs |
| `2025-11-25` | 64 | 183,957 | 346 / 548 µs | 168,601 | 377 / 468 µs |
| `2026-07-28` | 1 | 52,822 | 18 / 20 µs | 53,764 | 18 / 21 µs |
| `2026-07-28` | 16 | 135,574 | 115 / 207 µs | 147,030 | 107 / 161 µs |
| `2026-07-28` | 64 | 125,148 | 505 / 753 µs | 145,737 | 438 / 516 µs |

Where each SDK leads:

- On `2025-11-25`, TurboMCP has the higher throughput at every level: 30% more
  calls per second with one caller, 4% more at 16 and 9% more at 64. rmcp has
  the lower p99 at 16 and 64 callers.
- On `2026-07-28`, rmcp leads under concurrency: 8% more calls per second at 16
  callers and 16% more at 64, with a lower p99 at both. With one caller the two
  are within 2%. TurboMCP's throughput also drops between 16 and 64 callers,
  where rmcp's holds.

### Streamable HTTP, TurboMCP only

| Revision | C | Calls/s | p50 / p99 |
|---|---|---|---|
| `2025-11-25` | 1 | 7,173 | 138 / 160 µs |
| `2025-11-25` | 16 | 10,467 | 1,353 / 4,807 µs |
| `2025-11-25` | 64 | 10,839 | 5,673 / 10,607 µs |
| `2026-07-28` | 1 | 15,648 | 63 / 78 µs |
| `2026-07-28` | 16 | 71,511 | 213 / 400 µs |
| `2026-07-28` | 64 | 61,197 | 1,041 / 1,662 µs |

Stateful `2025-11-25` throughput stops near 10,800 calls a second however many
callers there are, and its latency grows with the queue. Stateless `2026-07-28`
scales to about 71,500 calls a second at 16 callers, then drops at 64.

### Why rmcp has no HTTP numbers

rmcp 3.11's Streamable HTTP client opened a new TCP connection for every call in
this loop, rather than reusing one. Each closed connection waits in `TIME_WAIT`
for 30 seconds on macOS, so at several thousand calls a second the run used up
the operating system's 16,384 local ports in under three seconds and every
later call failed. That happened with rmcp's default server settings and with
JSON responses and no sessions. A number from that run would measure the OS's
port range, so the table leaves rmcp out.

To include it anyway, set `TURBOMCP_LOAD_RMCP_HTTP=1`. The rmcp server setup is
`rmcp_http` in `crates/turbomcp-interop/fixtures/adders.rs`.

## Soak

One Streamable HTTP server under mixed traffic for four hours
(`crates/turbomcp/tests/soak.rs`), on the same machine and revision, from 00:05
to 04:05 ET on 2026-11-20. All of this ran at once, for the whole run:

- 16 tasks making stateless `2026-07-28` tool calls as fast as they could;
- 2 tasks churning stateful sessions, 10 a second each: connect, call once,
  then close, which sends the session's `DELETE`;
- 2 tasks calling a slow tool and cancelling the call a few milliseconds
  later, 10 a second each;
- 1 task opening `subscriptions/listen` streams and dropping them.

Every 10 seconds the harness recorded the process's resident memory, its open
file descriptors, and the calls, errors and latency of that window
(`target/soak/<time>.csv`). It fails on any error, and on memory or descriptors
that keep growing after the first quarter of the run.

| | |
|---|---|
| Result | passed |
| Tool calls | 853,335,904 (59,259 a second on average) |
| Stateful sessions opened and closed | 288,002 |
| Calls cancelled | 288,000 |
| Listen streams | 71,642 |
| Errors | 0 |

| Hour | Resident memory (median) | Calls/s | p50 | p99 |
|---|---|---|---|---|
| 1 | 79.1 MiB | 59,635 | 261 µs | 414 µs |
| 2 | 83.4 MiB | 59,061 | 262 µs | 415 µs |
| 3 | 83.4 MiB | 58,717 | 265 µs | 421 µs |
| 4 | 83.5 MiB | 59,625 | 261 µs | 414 µs |

Memory rose from 51 MiB at the first sample to 83.3 MiB by the end of the first
hour, as allocator pools and buffers filled, then stayed between 83.2 and 83.5
MiB for the remaining three hours. Open file descriptors held at 87-88 and fell
to 24 when the load stopped. Throughput and latency didn't drift from the first
hour to the last.

The steady traffic is stateless. Stateful traffic here is the session churn,
one call per session, so the soak doesn't load the stateful path the way the
throughput test does.

## Open work

These are the measured gaps we're working on before 4.0.0. This page will carry
before and after numbers from the same sweep on the same machine.

1. **Stateful HTTP throughput.** `2025-11-25` over HTTP stops near 10,800 calls a
   second regardless of concurrency.
2. **`2026-07-28` on the pipe under concurrency**, where rmcp is 8-16% faster and
   TurboMCP's throughput falls from 16 callers to 64.
3. **`2026-07-28` over HTTP at 64 callers**, where throughput falls from its
   16-caller peak.
