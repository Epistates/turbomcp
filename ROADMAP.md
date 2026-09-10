# Roadmap

Where TurboMCP is going, and what it is deliberately not doing. No dates —
this is maintained around a day job, and a date I can't keep is worse than none.
Order within a section is roughly the order things will be picked up.

Release candidate: **`4.0.0-alpha.4`**. The stable line is `3.x`.

## Shipped

The v4 alpha currently provides:

- **Three protocol revisions from one handler** — `2025-06-18`, `2025-11-25`,
  `2026-07-28` — each generated from its frozen upstream schema, with
  conversions that destructure exhaustively so a field added to one revision is
  a compile error until someone decides what the others do with it.
- **Both halves.** A macro-driven server and a typed client, verified against
  the official Rust SDK (`rmcp`) in both directions.
- **Transports:** stdio, Streamable HTTP, WebSocket.
- **Composition and visibility.** `Composite` mounts servers under a prefix or
  flat; `with_visibility` decides per caller which components exist at all.
- **`tower` middleware** at the frame seam — the dispatcher *is* a
  `tower::Service`.
- **Production seams:** OAuth 2.1 on both halves, identity-keyed rate limiting,
  OpenTelemetry traces and metrics, response caching, bidirectional elicitation.
- **Conformance in CI, both directions** — the official suite scores the server
  *and* the client on every push.

## Next

**Audit remediation and stable API review.** Track implementation and verification
in [the remediation record](docs/V4-AUDIT-REMEDIATION.md). Client authentication
conformance now uses the public OAuth coordinator; both failure baselines are
empty, and distinct successful checks are pinned in inventories.

**`4.0.0` stable.** Requires the security and lifecycle regressions to stay green,
reproducible performance measurements, public API review, and deployment
feedback. Passing conformance alone is insufficient release evidence.

**Deployment documentation.** See [deployment and migration](docs/DEPLOYMENT.md).

## Planned

**A proxy and CLI.** Aggregating N upstream MCP servers behind one endpoint,
with per-caller filtering — built on `Composite` at the capability level rather
than piping frames. No v4 proxy or CLI package is currently shipped. Remote authentication,
notification routing, cancellation, pagination, and upstream lifecycle need
explicit contracts before support can be claimed.

**The Apps extension** (SEP-1865), currently a skeleton crate.

**Fewer crates.** There are 16 workspace members and two excluded verification crates. Package boundaries must be settled before stable. Removing or moving a public
subcrate API is a breaking change even if the facade stays unchanged.

## Not planned

Saying no is part of a roadmap.

- **`2024-11-05` and `2025-03-26`.** Supporting five revisions is a real cost
  and `rmcp` already does it well. If you need those peers, use `rmcp` — the
  README says so too.
- **DPoP (RFC 9449).** Still absent from the MCP specification. An
  `Identity::Dpop` variant exists; a validator will follow the spec, not lead
  it.
- **Legacy SSE resumability (`Last-Event-ID` replay on the server).** A MAY on
  `2025-11-25` that `2026-07-28` removed entirely. Building it would serve one
  deprecating revision.
- **A distributed rate limiter or session store in-tree.** The `RateLimiter` and
  session traits are the seam; a Redis backend belongs in its own crate.

## Requests

If something here is in the wrong order for you, say so on an issue — real
usage moves things up. Requests for things in **Not planned** are still worth
filing if you have a use case I have not considered; that list is a current
judgement, not a policy.
