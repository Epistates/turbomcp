# turbomcp-server

The TurboMCP v4 server: `VersionDispatcher` (three supported protocol revisions, one handler surface), capability traits (`WithTools`/`WithResources`/`WithPrompts`/`WithCompletions`), `ServerBuilder`, sessions, core Tasks, subscriptions, progress/logging, MRTR client interaction, and the SEP-2549 cache policy.

Part of [TurboMCP](https://github.com/Epistates/turbomcp), a Rust SDK for the
[Model Context Protocol](https://modelcontextprotocol.io). Most users should
depend on the [`turbomcp`](https://crates.io/crates/turbomcp) facade, which
re-exports this crate's surface behind one dependency and its feature flags.

## Contracts and validation

See [deployment and migration](https://github.com/Epistates/turbomcp/blob/main/docs/DEPLOYMENT.md)
for authoritative catalog lookup, runtime schema validation, identity ownership,
OAuth network policy, trusted proxies, and default resource limits. See
[critical test coverage](https://github.com/Epistates/turbomcp/blob/main/docs/TESTING.md)
for the relevant regression and independent interoperability checks.

## License

MIT
