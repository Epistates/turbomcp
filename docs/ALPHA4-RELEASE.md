# 4.0.0-alpha.4 release preparation

Prepared 2026-09-09 from the audit remediation working tree. This is an alpha
candidate, not a published or tagged release. All 14 publishable crates use
4.0.0-alpha.4; private verification/codegen/Apps packages remain unpublished.
Internal dependency versions and all five lockfiles are aligned.

## Changes and migration

See [CHANGELOG.md](../CHANGELOG.md) for release notes and
[DEPLOYMENT.md](DEPLOYMENT.md) for provider lookup, session ownership,
subscription context, HTTP limits, and OAuth migration requirements. The crate
README now matches the repository's protocol and performance claims. A broken
OAuth rustdoc link and formatting in the excluded fuzz workspace were fixed
in this preparation pass. The conformance test module was moved after production
items to satisfy its excluded-workspace Clippy check. The justfile's Rust version now matches MSRV 1.88.

## Candidate verification

- All-feature workspace tests: 720 passed, zero failures or ignored tests.
- Doctests excluding generated protocol prose: 15 passed, 9 ignored.
- Workspace Clippy, all features and targets, warnings denied: passed.
- Excluded conformance and interoperability Clippy, all targets: passed.
- Formatting: workspace, conformance, interoperability, fuzz, and renamed fixture passed.
- All-feature example builds, renamed dependency fixture, foundation WASM check,
  and generated-code drift check: passed.
- Rust 1.88 all-feature workspace check: passed.
- docs.rs nightly configuration, warnings denied: passed. The generated protocol
  crate is excluded by the existing recipe because upstream schema prose is not
  valid Rust documentation markup. An additional broad stable rustdoc run
  confirmed this known limitation; it is not represented as passing.
- Strict client/server conformance: zero failures; 650/231 success messages.
  Corrected client fixtures and unmodified server report zero skips or warnings.
  [Fixture corrections and raw upstream mode](../crates/turbomcp-conformance/fixtures/README.md) remain explicit.
- rmcp interoperability: four tests passed.
- Package metadata, version consistency, file lists, dependency publish order:
  passed for all 14 publishable crates.
- Advisory database check: passed after refresh.

The socket-based tests were rerun with local network access after the sandbox
rejected listener binding. No checks were disabled to accommodate that failure.
Detailed local logs use `/tmp/turbomcp-alpha4-*.log`. Prior feature-matrix and
fuzz evidence is recorded in [V4-AUDIT-REMEDIATION.md](V4-AUDIT-REMEDIATION.md).

## Publication handoff

Review and commit the candidate, then publish in `just publish-order` order.
The guarded `just publish` recipe requires `CONFIRM=yes` and waits for index
availability between crates. Packaged sibling dependencies cannot be resolved
against crates.io until their new versions are published, so the package-list
check is not a claim that every registry tarball was independently built.
Tag and release notes should identify the final committed candidate. No commit,
tag, registry publication, or hosted release was created during preparation.

Longer performance/load/fuzz campaigns, broader
HTTP interoperability, and deployment pilots remain documented evidence gaps.
Apps is a placeholder and is excluded from publication and supported scope.

A subsequent [docs/test diligence pass](DOCS-TESTS-AUDIT.md) added proxy-chain,
OAuth concurrency/failure, JSON-RPC, and metadata-isolation coverage, corrected
publication tooling, and measured macro metadata caching. The current candidate
includes those fixes and passes 720 workspace tests.

Legacy POST-stream recovery and the corrected header/retry fixtures now pass;
the release gate reports 650 client and 231 server successes, zero failures,
skips, or warnings. Unmodified-upstream output remains separately reproducible.

The final diligence run also corrected a scheduling assumption in
`serve_concurrency::backpressure_caps_in_flight`: it now waits for handler
entry before checking overload, preserving the rejection and capacity assertions.

A deterministic teardown regression also fixed admission after pending-call
cleanup: the client closes its outbound receiver before draining pending calls,
so a request during a blocked transport close fails promptly instead of waiting
for its request timeout. `client_robustness` now includes this case.
