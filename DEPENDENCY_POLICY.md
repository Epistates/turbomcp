# Dependency policy

## The rule

Use battle-tested libraries; don't hand-roll. Before writing more than about a
hundred lines of anything with an established solution space, look for a crate
that already does it.

**Never hand-rolled, no exceptions:** cryptography, HTTP parsing, JSON
serialization, date and time handling, compression, IP/CIDR parsing, regular
expressions.

**Ours to write:** MCP protocol logic, the macro surface, version dispatch and
the conversions between protocol revisions, domain rules, and thin config
wrappers. This is the part that has no upstream to defer to.

## Choosing one

Weighed roughly in this order:

1. **Is it already in the tree?** Consistency beats a marginally better fit. Two
   crates doing the same job is a defect.
2. **Maintenance.** Recent releases, responsive issues, a real maintainer.
   Trusted publishers (`dtolnay`, `tokio-rs`, `BurntSushi`, the Rust project) are
   a strong signal.
3. **Cost against benefit.** A heavy dependency to replace a dozen lines is a
   bad trade. So is re-implementing a parser to save 200 KB.
4. **License.** MIT or Apache-2.0. Copyleft is not compatible with this
   project's MIT license and will be rejected by CI.
5. **Portability.** `turbomcp-core`, `-codec` and `-protocol` are `no_std` +
   `alloc` and must build for `wasm32-unknown-unknown`. A dependency that breaks
   that cannot go in those crates — the CI wasm step will catch it.

## Feature hygiene

- Internal crate dependencies use `default-features = false`, so enabling a
  feature in one crate never silently enables it in another.
- Optional functionality is feature-gated and off by default. The facade's
  `default = []`; only stdio is always linked.
- Feature combinations are tested by `cargo hack` in CI, so a feature that only
  compiles alongside another gets caught.

## Supply chain

`cargo-deny` runs on every push and checks four things: advisories, licenses,
banned crates, and sources. It is a hard gate.

- **Advisories.** A RUSTSEC advisory fails the build. Fixing it means updating,
  not adding an ignore. An ignore is a last resort, needs a comment saying why
  and what would remove it, and is reviewed rather than assumed permanent.
- **Sources.** crates.io only. No git or path dependencies in a published crate.
- **Duplicates.** Multiple major versions of one crate in the tree are worth
  removing when practical, but are not a hard failure — the ecosystem does not
  always allow it.

Untrusted-input decoders — the JSON-RPC codec, the `Mcp-Param` header sentinel,
URI templates — have `cargo-fuzz` targets, run out of band with `just fuzz`.

## Updates

Dependabot opens grouped PRs weekly for cargo and GitHub Actions. Patch and
minor updates are merged once CI is green. Major updates are reviewed on their
own, because they are the ones that move MSRV or change behaviour.

Security updates are not batched — they go in as soon as they are verified.

## Adding one

In the PR, say what it does, why an existing dependency or the standard library
does not cover it, and what its own dependency tree pulls in. `cargo tree
-p <crate>` in the description saves a reviewer the trip.
