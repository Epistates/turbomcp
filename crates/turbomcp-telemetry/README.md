# turbomcp-telemetry

TurboMCP v4 observability on the OpenTelemetry MCP semantic conventions: traces (`TraceContextLayer`: server spans named `{mcp.method.name} {target}`, W3C context via MCP `_meta`, identity and session id as keyed HMAC hashes) and metrics (`MetricsLayer`: `mcp.server.operation.duration` and an in-flight counter), with turnkey OTLP export behind the `otlp` feature.

Part of [TurboMCP](https://github.com/Epistates/turbomcp), a Rust SDK for the
[Model Context Protocol](https://modelcontextprotocol.io). Most users should
depend on the [`turbomcp`](https://crates.io/crates/turbomcp) facade, which
re-exports this crate's surface behind one dependency and its feature flags.

## License

MIT
