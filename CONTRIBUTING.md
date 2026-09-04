# Contributing to TurboMCP

Thanks for helping. Bug reports and small focused PRs are both welcome, and you
do not need to ask permission before opening either.

## Which branch

- **`main` is v4** (`4.0.0-alpha.x`), a ground-up rewrite. New work goes here.
- **`v3.x`** is the stable line. Fixes for `3.x` target that branch.

If you are not sure which your change belongs on, open the PR against `main` and
say so — retargeting is cheap.

## Getting set up

Rust **1.88** or newer, edition 2024. [`just`](https://github.com/casey/just)
runs everything:

```bash
just build          # development build
just check          # fast compile check
just test           # the full gate — run this before pushing
```

`just test` is what CI runs, in the same order: tests across the feature matrix,
`clippy -D warnings`, a no-default-features lint of the facade, `cargo fmt
--check`, a `wasm32-unknown-unknown` build of the `no_std` foundation crates,
and a docs.rs-configuration rustdoc build. If it passes locally it should pass
in CI.

Two suites live outside the workspace because they need something the main gate
should not depend on — a Node toolchain and a heavy `rmcp` dependency tree
respectively — so they have their own lockfiles and their own commands:

```bash
just conformance                                  # official MCP conformance, needs pnpm
cd crates/turbomcp-interop && cargo test          # cross-SDK interop against rmcp
just fuzz                                         # cargo-fuzz targets, nightly
```

**Check your toolchain against CI before trusting a green local run.** New
clippy lints in a newer stable have twice made a local gate pass while CI
failed; `rustup update stable` is usually the whole fix.

## Making a change

A few things about this codebase that are easy to trip over:

- **Handlers speak version-neutral types.** `turbomcp_protocol::neutral` is the
  handler-facing vocabulary; each protocol revision's wire types are generated
  from that revision's published schema and converted at the edges. Never make a
  handler speak a wire type.
- **The generated wire types are not editable.** Anything under
  `crates/turbomcp-protocol/src/v*/types.rs` is marked `@generated`. Change the
  schema or the generator and run `just codegen`.
- **A new field on one revision's wire is a deliberate compile error** in
  `v2025_06_18/convert.rs`, because the step-down conversions destructure
  exhaustively. That is the design — decide what the older revision does with
  the field rather than papering over it with `..`.
- **Capabilities are derived, not declared.** Writing a `#[resource]` is what
  advertises the `resources` capability. There is no capabilities builder.
- **Use `default-features = false` on internal crate dependencies** so features
  don't leak between crates.

For anything substantial, prefer a well-maintained crate over a hand-rolled
implementation — see [DEPENDENCY_POLICY.md](DEPENDENCY_POLICY.md). Never
hand-roll cryptography, HTTP parsing, JSON, or date/time handling.

## Tests

New behaviour needs a test that fails without it. Beyond that, the thing worth
knowing: **testing both halves of this SDK against each other is not enough.**
The client's missing server→client SSE stream survived two releases precisely
because our client was only ever tested against our server, which happens not to
exercise it. When a change touches wire behaviour, ask what a *different*
implementation would do, and reach for `just conformance` or the interop suite.

## Commit messages

Conventional-commit prefixes (`fix:`, `feat:`, `test:`, `refactor:`, `docs:`,
`chore:`), with a `!` for breaking changes. Explain **why** in the body — what
was wrong, and what it caused. The subject line says what changed; the body is
where a future reader finds out whether your reasoning still holds.

Please don't add authorship trailers or tool attribution. Your own
`Co-Authored-By` on a genuine co-author is welcome and will be preserved
through a squash merge.

## Pull requests

- Keep unrelated changes in separate commits; each commit should build and pass
  on its own.
- If you spot a real problem outside your change's scope, say so in the PR
  rather than folding a fix into it.
- Public API changes are gated by `cargo semver-checks` against the last
  published release. If the job flags a break, that is a conversation, not
  necessarily a blocker.

## Reporting bugs

The most useful report has the protocol revision, the transport, and either a
reproduction or the wire traffic. If you hit something with a specific MCP
client (Claude Desktop, an IDE, another SDK), naming it helps a lot — several
past bugs were only visible against a peer implemented differently from ours.

## Security

Please don't open a public issue for a vulnerability; see
[SECURITY.md](SECURITY.md).

## License

Contributions are licensed under the MIT license, matching the project.
