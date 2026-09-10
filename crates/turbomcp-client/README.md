# turbomcp-client

The TurboMCP v4 typed client: `ClientBuilder` + `ConnectMode` version negotiation, the neutral typed API, the MRTR input-required loop, task auto-driving, `#[mcp_header]` mirroring, and the SEP-2549 response cache with `list_changed` invalidation.

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
