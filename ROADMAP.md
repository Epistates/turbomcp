# Roadmap

Where TurboMCP is going, and what it is deliberately not doing. No dates —
this is maintained around a day job, and a date I can't keep is worse than none.
Order within a section is roughly the order things will be picked up.

Current release: **`4.0.0-alpha.2`**. The stable line is `3.x`.

## Shipped

The v4 rewrite is feature-complete for its own scope. In the tree today:

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

**Client auth conformance.** The conformance client runner does not yet drive
the OAuth flows, so ~20 `auth/*` scenarios sit in its expected-failure baseline.
The client implements OAuth 2.1 already (`client-oauth`); this is wiring, and it
is the largest remaining gap against `rmcp`.

**`4.0.0` stable.** Gated on the alpha finding real users and their bugs. The
API is where I want it; what's missing is exposure. If you are running an alpha,
opening an issue is the single most useful thing you can do for this line.

**Documentation.** The README and rustdoc are thorough; a guide that walks
somebody from zero to a deployed server is not there yet.

## Planned

**A proxy and CLI.** Aggregating N upstream MCP servers behind one endpoint,
with per-caller filtering — built on `Composite` at the capability level rather
than piping frames. Bridging a single server is a commodity (supergateway,
mcp-proxy); aggregation is the part nobody has done well. Scoped, with several
open design questions still to settle.

**The Apps extension** (SEP-1865), currently a skeleton crate.

**Fewer crates.** Seventeen is more than this needs. Consolidating toward ~10
without changing the facade's surface is a `4.x` minor at most, but it is
disruptive to anyone importing sub-crates directly, so it needs care.

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
