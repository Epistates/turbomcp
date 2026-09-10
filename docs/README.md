# v4 documentation

The active candidate is 4.0.0-alpha.4. The codebase provides the library/server
and typed client, three protocol revisions, stdio/HTTP transports, WebSocket as
a convenience transport, OAuth, telemetry, and Tasks. There is no v4 standalone
MCP proxy, CLI, OpenAPI adapter, or implemented Apps extension. Trusted HTTP
reverse-proxy configuration is supported and is a separate feature.

- [Quickstart and package map](../README.md)
- [Deployment, limits, auth, and alpha migration](DEPLOYMENT.md)
- [v3 migration](../crates/turbomcp/MIGRATION.md)
- [Contributor commands](../CONTRIBUTING.md)
- [Versioning and public API policy](../VERSIONING.md)
- [Security reporting](../SECURITY.md)
- [Docs/test audit and measured optimization](DOCS-TESTS-AUDIT.md)
- [Critical test coverage and limits](TESTING.md)
- [Actual benchmark targets and methodology](../benches/README.md)
- [Release checklist](../scripts/RELEASE_CHECKLIST.md)
- [Alpha.4 verification snapshot](ALPHA4-RELEASE.md)
- [Audit remediation evidence](V4-AUDIT-REMEDIATION.md)

Files in audits/ and bugs/ are historical records. Their findings, test counts,
package names, and completion claims must not be read as current guarantees.
Release verification documents are dated snapshots, not live test counts.
