# v4 documentation

The current release is 4.0.0-beta.1, the API we intend to ship as 4.0.0. It
provides the server and typed client, three protocol revisions, stdio and
Streamable HTTP (with WebSocket as a route on the same endpoint), OAuth on both
sides, telemetry, Tasks, Server Cards, and the gateway (`turbomcp-proxy`, and
the `turbomcp` command) that serves remote MCP servers beside local tools.
There is no OpenAPI adapter, and the Apps extension is not implemented.

- [Quickstart and package map](../README.md)
- [Deployment, limits, auth, and prerelease migration](DEPLOYMENT.md)
- [v3 migration](../crates/turbomcp/MIGRATION.md)
- [Contributor commands](../CONTRIBUTING.md)
- [Versioning and public API policy](../VERSIONING.md)
- [Security reporting](../SECURITY.md)
- [Critical test coverage and limits](TESTING.md)
- [Measured performance, and how to reproduce it](PERFORMANCE.md)
- [Actual benchmark targets and methodology](../benches/README.md)
- [Release checklist](../scripts/RELEASE_CHECKLIST.md)
