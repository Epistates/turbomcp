# turbomcp-macros

The TurboMCP v4 procedural macros: `#[server]`, `#[tool]`, `#[resource]`, `#[prompt]`, `#[completion]`, and `#[mcp_header]`. The macro derives capability advertisement and schema-generation code from signatures. Tool metadata initializes once at runtime and returns independent copies; dynamic catalogs use custom providers. Renamed facade dependencies are supported.

Part of [TurboMCP](https://github.com/Epistates/turbomcp), a Rust SDK for the
[Model Context Protocol](https://modelcontextprotocol.io). Most users should
depend on the [`turbomcp`](https://crates.io/crates/turbomcp) facade, which
re-exports this crate's surface behind one dependency and its feature flags.

## License

MIT
