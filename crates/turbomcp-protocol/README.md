# turbomcp-protocol

The TurboMCP v4 protocol layer: version-neutral handler types (`neutral`), the generated `2025-06-18`, `2025-11-25`, and `2026-07-28` wire modules, and the total conversions between them. Handlers speak neutral types; wire shapes are edge conversions. wasm32-portable.

Part of [TurboMCP](https://github.com/Epistates/turbomcp), a Rust SDK for the
[Model Context Protocol](https://modelcontextprotocol.io). Most users should
depend on the [`turbomcp`](https://crates.io/crates/turbomcp) facade, which
re-exports this crate's surface behind one dependency and its feature flags.

## License

MIT
