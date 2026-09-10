# Client fixture corrections v1

The release gate uses the official `@modelcontextprotocol/conformance@0.2.0-alpha.11`
harness with three explicit fixture corrections. It is not an unmodified upstream
run. `corrected-client.mjs` verifies the bundled source SHA-256 before copying it
to a temporary directory; each replacement must match exactly once. Dependencies
come from the pinned package environment; the cache is never modified. Assertions,
tolerances, expected headers, and failure dispositions are unchanged.

Upstream bundle SHA-256:
`a10085d0cfc9dd9192cc227f0f4dd6f1af9a94f6a0d3e30af08d4a0bcf268aae`.

1. Legacy SSE retry mock: its initialize response negotiates `2025-11-25`, the
   revision the scenario is scored against, instead of hardcoded `2025-03-26`.
   This exposes the actual POST-stream recovery path in the supported client.
2. Modern HTTP headers: the method inventory expects `server/discover` instead
   of `initialize` and `notifications/initialized`, which the modern revision
   removed. It still checks tools, resources, prompts, and the three name headers.
3. The shared mock records discovery headers before returning its discovery
   response. Previously this early return bypassed header checks altogether.

This fixes fixture applicability; it does not turn a skip into a pass. The modern
client must actually send discovery with its correct header. The Rust runner
also requires every applicable method-specific label, because upstream shares
one check id across several methods.

The SDK additionally needed a real fix: resume a gracefully closed legacy POST
SSE response through GET with that request's Last-Event-ID after its retry delay.
An early standalone GET alone did not prove this behavior. The request's deadline,
cancellation, session, bearer source, byte limits, and final-response ownership
remain in force. Four Rust integration regressions check concurrent cursors,
retry timing, authentication/session headers, no repeated POSTs, cancellation,
and rejection of retry delays over the recovery limit rather than reconnecting early.

Both corrected client and unmodified server gates require a successful harness
exit, exact inventories, and a completion summary. Corrected client results:
650 success messages, 488 distinct scenario/check pairs, zero failures/skips/warnings.
Server results: 231 success messages, 215 distinct pairs, zero failures/skips/warnings.

For reproducibility, run the unmodified upstream client:

```sh
TURBOMCP_CONFORMANCE_STRICT=1 TURBOMCP_CONFORMANCE_UPSTREAM=1 cargo test --manifest-path crates/turbomcp-conformance/Cargo.toml --test conformance_client -- --nocapture
```

That mode uses its own exact inventory and still exposes two inapplicable modern
header skips plus two unexercised legacy SSE warnings. It is never reported as a
clean corrected run. Remove these corrections only after a reviewed upstream
package update supplies equivalent fixtures. Do not broaden SDK revision support,
weaken assertions, or manufacture obsolete RPCs merely to satisfy an old fixture.

Upstream source and integration contract:
https://github.com/modelcontextprotocol/conformance
