# Security Policy

## Reporting a vulnerability

**Do not open a public issue for a security problem.**

Report it privately through
[GitHub Security Advisories](https://github.com/Epistates/turbomcp/security/advisories/new),
or by email to **nick@epistates.com**.

Please include enough to reproduce it: the crate and version, the feature flags
in play, the protocol revision if it matters, and a minimal server or client
that shows the behaviour. A JSON-RPC frame or HTTP request that triggers it is
worth more than a description of it.

You can expect an acknowledgement within 3 working days and an assessment
within 10. If a report is accepted we will agree a disclosure date with you,
publish a GitHub Security Advisory with a CVE, and credit you unless you would
rather we didn't. If it is declined you will get the reasoning, not silence.

## Supported versions

| Version | Supported |
|---|---|
| `4.0.x` (pre-release) | Yes — fixes land on `main` |
| `3.1.x` | Security fixes only |
| `< 3.1` | No |

`4.0.0-alpha`/`-beta`/`-rc` releases are pre-release software. Report
vulnerabilities in them as normal, but expect fixes as a new pre-release rather
than a patch to an existing one.

## Scope

In scope — anything reachable by a peer over the protocol:

- **Parsing untrusted input**: the JSON-RPC codec, the `Mcp-Param` header
  codec, URI-template matching, and the wire-type deserializers. These are the
  crate's fuzz targets for exactly this reason.
- **Authentication and authorization**: bearer-token validation, JWKS
  retrieval and caching, the RFC 9728 protected-resource metadata endpoint, the
  OAuth 2.1 client flow, `#[tool(scopes(…))]` enforcement, and visibility
  policies. A component that a policy hides must be unreachable, not merely
  absent from a listing.
- **Transport handling**: HTTP session management and termination, `Origin`
  and DNS-rebinding checks, `X-Forwarded-For` handling behind a trusted proxy,
  rate limiting, request size limits, and WebSocket framing.
- **Cross-session leakage**: anything that lets one session observe or affect
  another's state, subscriptions, or MRTR turns.
- **Telemetry**: PII reaching spans or metrics that the redaction layer is
  documented to strip.

Out of scope:

- Vulnerabilities in a server *you* write with this SDK — a `#[tool]` that
  shells out on unvalidated input is your bug, not the framework's. We will
  still take reports where the framework's API made the unsafe path the
  obvious one.
- Denial of service through resource exhaustion that an operator can bound with
  the configuration the crate already provides (rate limits, size limits,
  session timeouts). Report it if the bound doesn't work, or if there is no
  bound to set.
- The `turbomcp-interop` and `turbomcp-conformance` crates. Both are test
  harnesses, excluded from the workspace, and not published.
- Anything requiring an attacker who already has code execution in the server
  process.

## What this crate does to reduce its own attack surface

Stated so you know what to hold us to, not as a guarantee:

- **`#![forbid(unsafe_code)]` in every published crate.** There is no `unsafe`
  block to audit, and adding one is a compile error rather than a review
  question.
- **Continuous fuzzing** of the untrusted-input decoders, on every push.
- **`cargo-deny`** on every push for RUSTSEC advisories, license policy, and
  source provenance.
- **A conformance suite** run against the official MCP conformance harness, and
  cross-SDK interop tests against the official Rust SDK.
- **Dependencies are chosen, not accumulated** — cryptography, HTTP parsing,
  JSON, date/time, and IP/CIDR handling are delegated to established crates
  rather than reimplemented.

## Security-relevant configuration

Several protections are opt-in because they depend on your deployment. If you
expose a server beyond localhost, review at minimum: `HttpConfig`'s
authenticator, rate limiter, and trusted-proxy settings; the WebSocket
transport's `Origin` allowlist; and `ServerBuilder::with_visibility` if some
components should not be reachable by every caller. The crate's HTTP transport
binds where you tell it to and does not assume a reverse proxy in front.
