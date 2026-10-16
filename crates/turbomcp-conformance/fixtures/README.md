# Client fixture corrections v2

The release gate uses the official `@modelcontextprotocol/conformance@0.2.0-alpha.12`
harness with two explicit fixture corrections. It is not an unmodified upstream
run. `corrected-client.mjs` verifies the bundled source SHA-256 before copying it
to a temporary directory; each replacement must match exactly once. Dependencies
come from the pinned package environment; the cache is never modified. Assertions,
tolerances, expected headers, and failure dispositions are unchanged.

Upstream bundle SHA-256:
`b56fb575e61edaac83c2008af40a6a78b47815c4038a02a9b04cd336d38832a0`.

1. Modern HTTP headers: the method inventory expects `server/discover` instead
   of `initialize` and `notifications/initialized`, which the modern revision
   removed. It still checks tools, resources, prompts, and the three name headers.
2. The shared mock records discovery headers before returning its discovery
   response. Previously this early return bypassed header checks altogether.

v1 also corrected the legacy SSE retry mock, which negotiated `2025-03-26`
instead of the `2025-11-25` it is scored against; alpha.12 fixed that
upstream, so the correction is gone.

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
701 passed checks, 534 distinct scenario/check pairs, zero failures/skips/warnings.
Server results: 232 passed checks, 216 distinct pairs, zero failures/skips/warnings.
After a harness update, `TURBOMCP_CONFORMANCE_BLESS=1` rewrites the inventories
from a passing run; review that diff like any other.

For reproducibility, run the unmodified upstream client:

```sh
TURBOMCP_CONFORMANCE_STRICT=1 TURBOMCP_CONFORMANCE_UPSTREAM=1 cargo test --manifest-path crates/turbomcp-conformance/Cargo.toml --test conformance_client -- --nocapture
```

That mode uses its own exact inventory and passes with zero failures and zero
warnings; it still exposes two inapplicable modern header skips. It is never reported as a
clean corrected run. Remove these corrections only after a reviewed upstream
package update supplies equivalent fixtures. Do not broaden SDK revision support,
weaken assertions, or manufacture obsolete RPCs merely to satisfy an old fixture.

Upstream source and integration contract:
https://github.com/modelcontextprotocol/conformance
