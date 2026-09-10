# v4 audit remediation

> Subsequent update: legacy recovery and fixture gaps are addressed in the
> [conformance correction record](../crates/turbomcp-conformance/fixtures/README.md).
> Counts and warnings below describe the earlier audit snapshot.

Baseline: `2cedc9f` (includes HTTPS endpoint validation and the standalone
HTTP stream startup fix). Both conformance failure baselines are empty. The
previous attribution of the SSE retry failure solely to the upstream harness
was incorrect: stream startup ordering was a client defect.

This checklist tracks implementation and evidence, not release certification.

- [x] F1/F2/F10: authoritative catalog lookup, fail-closed visibility, schema
  validation, and fresh header metadata across pagination and catalog changes.
- [x] F3/F4/F9: bounded admission, cancellation-safe client registration,
  supervised background work, and deadline-bounded shutdown.
- [x] F5: structured HTTP errors and public OAuth orchestration.
- [x] F6/F7/F8: JWT time validity, redirect-safe bounded auth networking,
  configurable private-network policy, and coalesced JWKS refresh.
- [x] Bind legacy sessions and identity-sensitive state to their creating
  principal; anonymous sessions remain anonymous.
- [x] F11: successful benchmark fixtures and reproducible performance evidence.
- [x] F12: current docs and strict conformance coverage accounting.
- [x] Review macro dependency renaming, schema normalization, JSON-RPC
  envelopes, and public API evolution.
- [x] Verify regression tests, conformance, interoperability, Clippy, MSRV,
  feature combinations, and relevant fuzz/load checks.

Private/internal authorization servers remain supported. Public-network-only
egress is an explicit operator policy, independent of HTTPS enforcement.


## Implementation

F1/F2/F10 share authoritative provider lookup. Default lookups traverse bounded
pagination; macros supply direct lookup. Flat mounts route beyond page one and
reject ambiguous ownership. Visibility and lookup failures deny invocation.
Input and structured output schemas are enforced, including task augmentation;
header mirrors use the current definition and client recovery bypasses stale caches.

F3/F4/F9 bound application/control admission, pending calls, callbacks, HTTP
requests, response bytes, and shutdown. Client deadlines include queueing and
sending. Connection and transport lifetimes own their background work; close
cancels and waits. Partial writes are never followed by another frame after
cancellation. HTTP admission happens before authentication.

F5 preserves structured HTTP failures and challenges. The public OAuthSession
implements coordinated authorization, refresh, scope changes, and rediscovery;
the conformance runner now exercises this API. F6/F7/F8 enforce nbf, bound auth
networking, disable redirects/proxies by default, coalesce JWKS refresh, and
establish failure cooldowns. Public-only egress is opt-in and validates DNS
addresses at connection resolution. Custom HTTP clients are explicit policy
overrides.

Sessions, task state, rate limits, and MRTR use issuer/subject identity. MRTR
also binds method and arguments. Mutable bearer HTTP caching is disabled.
Independent issuers can use independent JWT validators/key sources. Additional
fixes cover strict JSON-RPC envelopes, redacted OAuth callback debug output,
renamed facade dependencies in macros, and conservative allOf normalization.

Public API changes and operator settings are documented in [DEPLOYMENT.md](DEPLOYMENT.md).
These changes target the alpha; public subcrate compatibility matters when
freezing the release. Apps remains explicitly unsupported rather than being
advertised as an implemented extension.

## Verification, 2026-09-09

Results apply to the uncommitted remediation working tree based on `2cedc9f`.
Toolchain: rustc 1.98.0 (88d9e12ae, 2026-08-18), aarch64-apple-darwin,
LLVM 22.1.8. Separate minimum supported Rust verification used 1.88.

| Check | Result |
| --- | --- |
| Locked offline workspace tests, all features, lib/bins/tests | 709 passed, 0 failed, 0 ignored across 100 test binaries |
| Workspace Clippy, all features/targets, warnings denied | Passed |
| Rust 1.88 workspace check, all features | Passed |
| Foundation no-default-feature tests | 95 passed |
| Foundation no-default-feature wasm32 build | Passed |
| Workspace doctests, excluding generated protocol crate | 15 passed, 9 ignored |
| Facade feature powerset, depth 2, without dev dependencies | 43 combinations passed in an isolated source snapshot |
| Renamed dependency fixture | Passed; CI check added |
| Generated protocol drift (`just codegen-check`) | Passed for all three schemas |
| Existing rmcp interoperability suite | 4 passed |
| Supply chain | Advisories, bans, licenses, and sources passed with all features; yanked chacha20 version replaced and MIT-0 dependency license explicitly allowed |
| Strict client conformance | 647 success messages, 486 distinct scenario/check pairs; 0 failures |
| Strict server conformance | 231 success messages, 215 distinct scenario/check pairs; 0 failures |

The pinned harness is `@modelcontextprotocol/conformance` 0.2.0-alpha.11.
Both failure baselines remain empty. Exact successful, skipped, and warning
inventories supplement pass floors and completion/exit checks. Client results
include two modern header skip messages and two reviewed legacy SSE warnings;
server results contain neither skips nor warnings. The legacy retry fixture
negotiates unsupported `2025-03-26`, preventing its retry-timing and Last-Event-ID
checks from executing. This is separate from the genuine modern stream startup
race, whose fix is preserved and whose conformance checks pass.

Raw client success counts decreased from the maintainer's 732 because migrating
to public OAuth orchestration removed duplicate probes. Repeated messages are
not independent coverage. The inventories record distinct scenario/check pairs.

Four 30-second ASan fuzz smoke campaigns completed without crashes:
codec decode 1,889,661 executions; sonic differential 418,539; MCP header codec
5,592,919; URI template 19,107. These are smoke tests, not sustained campaigns.

## Performance evidence and limits

Both benchmark fixtures now assert the successful result before timing.
The dispatch fixture includes required client capabilities. Short Criterion runs
used 10 samples, 1-second warmup, and 2-second measurement. Dispatch tools/call
measured a 7.6482 microsecond point estimate (interval 7.6111–7.6955).
The legacy 2025-11-25 duplex roundtrip comparison measured TurboMCP 26.572
microseconds (26.119–26.803), rmcp 25.583 (25.475–25.770). Each SDK uses its own
client and server; handshake is outside timing. These results do not establish
a TurboMCP speed advantage. Concurrent build activity and unrecorded hardware
model prevent treating this short run as a controlled performance baseline.
Criterion comparisons against old local samples are not valid audit baselines.

Local raw logs are `/tmp/turbomcp-remediation-bench.log` and
`/tmp/turbomcp-remediation-comparison.log`. Other validation logs share the
`/tmp/turbomcp-remediation-` prefix; full workspace tests are recorded at
`/tmp/turbomcp-v4-audit-tests.log`. These temporary logs are not durable CI artifacts.

## Remaining release evidence

The reproduced defects and listed implementation reviews are addressed. The
following broader acceptance goals remain open and must not be represented as
completed by the fixes or short checks above:

- Controlled HTTP/stdio performance matrices, allocation and memory measurements,
  sustained load/soak runs, cancellation storms, and long fuzz campaigns.
- Python and rmcp HTTP interoperability beyond the current TypeScript conformance
  and rmcp duplex suite; supported-revision legacy SSE replay/timing evidence.
- Real deployment pilots, final API/package freeze, and stable release review.
- Apps implementation, examples, and its own compatibility/security evidence if
  that extension is promoted into supported scope.

This report closes defect remediation; it does not certify state-of-the-art
performance or stable production readiness.
