# turbomcp-service

The TurboMCP v4 service seam: the tower-shaped `McpService` (`Service<JsonRpcMessage>`), the `Transport` trait with its seven-invariant parity contract, the concurrent `serve()` driver (bounded admission, separate control budget, deadline-bounded writes and shutdown), and the transport-level seams (`HttpAuthenticator`, `RateLimiter`, `SessionTerminator`).

Part of [TurboMCP](https://github.com/Epistates/turbomcp), a Rust SDK for the
[Model Context Protocol](https://modelcontextprotocol.io). Most users should
depend on the [`turbomcp`](https://crates.io/crates/turbomcp) facade, which
re-exports this crate's surface behind one dependency and its feature flags.

## License

MIT
