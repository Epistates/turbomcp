# v4 release checklist

1. Review the candidate diff, public API changes, and supported protocol scope.
2. Align publishable manifests, internal version requirements, and all five
   lockfiles (root, conformance, interop, fuzz, renamed-dependency fixture).
3. Update CHANGELOG.md, README status, migration guidance, and ROADMAP.md.
4. Run `just test`, excluded conformance/interop tests and Clippy, and
   `cargo deny check advisories`. Review warnings/skips as well as failures.
5. Run `just publish-check` for metadata, package lists, and dependency order.
   This is not a registry tarball build: unpublished sibling versions cannot
   yet resolve on crates.io. Review package contents and final CI status.
6. Review and commit the exact candidate. Only then authorize publication with
   `CONFIRM=yes just publish`. It stops at the first failed crate; diagnose a
   partial release before resuming, never silently skip arbitrary failures.
7. Tag the published commit and use the reviewed changelog for release notes.

`scripts/publish_all.sh` is a compatibility wrapper: dry-run by default;
`DRY_RUN=false` delegates to the same guarded publication recipe. It does not
skip verification, maintain a second package list, or ignore failed publishes.

See [alpha.4 handoff](../docs/ALPHA4-RELEASE.md) for the candidate's evidence and
[versioning policy](../VERSIONING.md) for compatibility rules.
