# Roadmap

Where TurboMCP is going, and what it is deliberately not doing. No dates —
this is maintained around a day job, and a date I can't keep is worse than none.
Order within a section is roughly the order things will be picked up.

Current release: **`4.0.0-beta.2`**, the API we intend to ship as `4.0.0`
(see [VERSIONING.md](VERSIONING.md)). The stable line is `3.x`.

## Shipped

The v4 beta provides:

- **Three protocol revisions from one handler** — `2025-06-18`, `2025-11-25`,
  `2026-07-28` — each generated from its frozen upstream schema, with
  conversions that destructure exhaustively so a field added to one revision is
  a compile error until someone decides what the others do with it.
- **Both halves.** A macro-driven server and a typed client, verified against
  the official Rust, TypeScript, Python and Go SDKs.
- **Transports:** stdio, Streamable HTTP, and WebSocket as a route on the HTTP
  endpoint. The HTTP endpoint is a `tower` service that mounts in any axum app.
- **Composition and visibility.** `Composite` mounts servers under a prefix or
  flat; `with_visibility` decides per caller which components exist at all.
- **A gateway.** `turbomcp-proxy` serves remote MCP servers (stdio, HTTP,
  WebSocket) beside local tools behind one endpoint, with OAuth on both sides,
  and the `turbomcp` command runs it from an mcpServers file.
- **`tower` middleware** at the frame seam — the dispatcher *is* a
  `tower::Service`.
- **Production seams:** OAuth 2.1 on both halves, identity-keyed rate limiting,
  OpenTelemetry traces and metrics, response caching, bidirectional elicitation,
  Tasks, and Server Cards.
- **Conformance in CI, both directions** — the official suite scores the server
  *and* the client on every push.

## Next

**`4.0.0` stable.** The API is frozen as of beta.1. What remains is evidence:
reproducible HTTP and stdio performance numbers, soak and cancellation-storm
runs, longer fuzz campaigns, and real deployments. The first performance
sweep and soak are in [docs/PERFORMANCE.md](docs/PERFORMANCE.md). Breaking
changes from here are for defects only, and the CHANGELOG calls each one out.

**The next MCP revision.** The specification moves to a release every six
months, and the next one is due in late March 2027. Support for it lands as a
`4.x` minor release, alongside the three revisions served today.

**Deployment documentation.** See [deployment and migration](docs/DEPLOYMENT.md).

## Planned

**The Apps extension** (SEP-1865), now Stable upstream. It isn't implemented
yet and will land in a `4.x` minor.

## Not planned

Saying no is part of a roadmap.

- **`2024-11-05` and `2025-03-26`.** Supporting five revisions is a real cost
  and `rmcp` already does it well. If you need those peers, use `rmcp` — the
  README says so too.
- **DPoP (RFC 9449).** Still absent from the MCP specification. Support will
  follow the spec, not lead it; `Identity` is non-exhaustive, so a DPoP
  variant can be added in a minor release.
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
