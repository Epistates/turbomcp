# Versioning

TurboMCP follows [Semantic Versioning](https://semver.org/). This document says
what that means here, because an MCP SDK carries three moving version numbers
that people confuse: the crate version, the MCP protocol revisions it speaks,
and the Rust it needs.

## Crate versions

Every crate in the workspace shares one version and they are released together.
A `turbomcp` at `4.0.0` depends on `turbomcp-core` at `4.0.0`; mixing versions
across the family is not supported.

| Change | Bump |
|---|---|
| Breaking public API change | major |
| New API, backward compatible | minor |
| Fix with no API change | patch |
| Newly supported protocol revision | minor |
| Dropped protocol revision | major |
| MSRV increase | minor |

Prereleases are `-alpha.N` / `-beta.N` / `-rc.N`. **A prerelease may break API
against the prerelease before it** — that is what the channel is for. `4.0.0`
starts the compatibility promise.

`cargo semver-checks` runs in CI against the last published release, so an
accidental break is caught before it ships rather than after.

## Release lines

| Line | Branch | Status |
|---|---|---|
| `4.x` | `main` | Prerelease (`4.0.0-alpha.x`). Active development. |
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
  only ship dated, frozen revisions on the stable channel; the pre-freeze
  `Draft` names survive as deprecated aliases and are not a supported surface.

## Minimum supported Rust version

**MSRV is 1.88**, edition 2024, and it is checked in CI.

Raising it is a **minor** version bump, not a patch. We raise it only for a
concrete reason — a language or standard-library feature the codebase actually
uses, or a dependency that raised its own — and never in a patch release.

## Deprecations

An item is marked `#[deprecated]` with a note pointing at the replacement, and
stays for at least one minor release before removal in the next major. If you
see a deprecated item with no stated replacement, that is a bug; please report
it.

## What is not covered

These are excluded from the compatibility promise:

- Anything `#[doc(hidden)]`, or documented as internal.
- The exact text of error messages and log output.
- `_meta` keys under `io.turbomcp/*`, which are implementation detail unless
  documented as public.
- The generated wire types' internal module layout — use `turbomcp::neutral`.
- Crates marked `publish = false` (`turbomcp-codegen`, and the interop and
  conformance test crates).
