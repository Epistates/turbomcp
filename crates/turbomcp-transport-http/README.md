# turbomcp-transport-http

The TurboMCP v4 Streamable HTTP transport (axum 0.8): POST + per-request SSE upgrade, the legacy GET stream, sessions (`Mcp-Session-Id`, DELETE termination), Origin/Host guards, and the auth/rate-limit seams.

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
