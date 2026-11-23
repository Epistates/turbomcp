# Versioning

TurboMCP follows [Semantic Versioning](https://semver.org/). This document says
what that means here, because an MCP SDK carries three moving version numbers
that people confuse: the crate version, the MCP protocol revisions it speaks,
and the Rust it needs.

## Crate versions

Publishable crates in the workspace share one version and are released together.
Private codegen and verification packages are not published.
A `turbomcp` at `4.0.0` depends on `turbomcp-core` at exactly `4.0.0`: each
crate requires its siblings with `=`, so cargo never mixes versions across the
family. That is what lets the crates share hidden items with each other.

| Change | Bump |
|---|---|
| Breaking public API change | major |
| New API, backward compatible | minor |
| Fix with no API change | patch |
| Newly supported protocol revision | minor |
| Dropped protocol revision | major |
| MSRV increase | minor |

Prereleases are `-alpha.N` / `-beta.N` / `-rc.N`. An alpha may break API
against the one before it; that is what the channel is for. **From
`4.0.0-beta.1`, the API is the one we intend to ship as `4.0.0`**: a later beta
or release candidate breaks it only to fix a defect, and the CHANGELOG calls
each one out. `4.0.0` starts the full compatibility promise.

`cargo semver-checks` runs in CI against the last published release, so an
accidental break is caught before it ships rather than after.

## Release lines

| Line | Branch | Status |
|---|---|---|
| `4.x` | `main` | Prerelease (`4.0.0-beta.x`). API frozen except for defects. |
| `3.x` | `v3.x` | Stable. Fixes and backward-compatible additions. |

`3.x` is maintained for bug fixes and low-risk additions. It will get security
fixes for at least six months after `4.0.0` is released. New features land on
`4.x`.

## Protocol revisions

The crate version and the MCP protocol revision are independent. TurboMCP `4.x`
serves three published revisions from one handler:

| Revision | Shape |
|---|---|
| `2025-06-18` | stateful |
| `2025-11-25` | stateful |
| `2026-07-28` | stateless |

Each is generated from that revision's frozen upstream schema. Pin the set a
server answers with `#[server(protocols("2025-11-25", …))]`.

Some things worth being explicit about:

- **Adding a revision is a minor bump.** It is additive: existing clients keep
  negotiating what they already negotiated.
- **Removing one is a major bump**, because a peer that could connect can no
  longer connect.
- **`2024-11-05` and `2025-03-26` are not served and will not be.** If you need
  them, use [`rmcp`](https://github.com/modelcontextprotocol/rust-sdk), which
  does.
- A revision that upstream marks as draft may change shape before it freezes. We
  only ship dated, frozen revisions.

## Minimum supported Rust version

**MSRV is 1.95**, edition 2024, and CI checks it.

The MSRV rolls, as tokio's does: a release may raise it to a Rust version
released at least six months before. A raise comes in a minor or major
release, never a patch, and the CHANGELOG says so. It keeps the
dependencies on their current releases, which raise their own floors over
time. The unpublished codegen tool may need a newer Rust than this.

## Deprecations

An item is marked `#[deprecated]` with a note pointing at the replacement, and
stays for at least one minor release before removal in the next major. If you
see a deprecated item with no stated replacement, that is a bug; please report
it.

## What is not covered

These are excluded from the compatibility promise:

- Anything `#[doc(hidden)]`, or documented as internal, including the
  `__private` modules the crates share with each other.
- The generated wire modules, `turbomcp_protocol::v2025_06_18`, `v2025_11_25`
  and `v2026_07_28`. They are hidden and track the upstream schema, so an
  erratum or a codegen fix can change them in any release. The `neutral`
  types are the contract; convert through their `From` impls.
- The exact text of error messages and log output.
- `_meta` keys under `io.turbomcp/*`, which are implementation detail unless
  documented as public.
- Crates marked `publish = false` (`turbomcp-codegen`, `turbomcp-wire-check`,
  and the interop and conformance test crates).

These are covered, but may break in a `4.x` minor release, said so in its
CHANGELOG:

- **Extensions that are Beta upstream.** `turbomcp-ext-tasks` (the Tasks
  extension, SEP-2663) and Server Cards (`turbomcp_protocol::server_card` and
  the card route) implement extensions the MCP project classes as Beta
  (SEP-3392): they may still change, and we follow them.
- **`turbomcp-telemetry`'s OpenTelemetry surface.** It has to use the same
  `opentelemetry` release as the application for the global providers to
  work at all, so it follows `opentelemetry`'s 0.x releases.

## Public dependencies

A few third-party types appear in TurboMCP's API on purpose. A breaking
release of any of them is a breaking release of TurboMCP:

- `serde`, `serde_json`, `bytes`, `tokio`, `futures-core`, `http`,
  `http-body`, `tower-service` / `tower-layer`, `schemars`, `zeroize`,
  `ipnet` and `tracing`: all at 1.x, or long-stable 0.x.
- `reqwest`, which is how a client brings its own proxy, root certificates
  and client certificate. `turbomcp-auth`, `turbomcp-transport-http` and
  `turbomcp::client` re-export it, so use theirs.

Anything else is wrapped in a TurboMCP type, so its releases don't reach you:
JWT keys and algorithms (`turbomcp_auth::{Algorithm, VerifyingKey,
SigningKey}`), the HTTP server (`HttpService`, mountable in any axum), the
WebSocket client (it takes an `http::Request`), rate-limit quotas and
cancellation tokens.
