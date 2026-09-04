//! Official MCP conformance suites, run against TurboMCP's server *and* client.
//!
//! The crate hosts two suites that drive the same Node CLI in opposite
//! directions:
//!
//! - `tests/conformance_server.rs` — the harness is the client, connecting to
//!   an in-process TurboMCP Streamable-HTTP server.
//! - `tests/conformance_client.rs` — the harness is the server, spawning the
//!   `conformance-client` binary against a mock server per scenario.
//!
//! [`harness`] is what they share: the pinned package, the results parser, and
//! the baseline scoring. It is `exclude`d from the parent workspace so the Node
//! dependency never touches the main lockfile/gate; run it on its own:
//!
//! ```text
//! just conformance                                    # from the repo root
//! cd crates/turbomcp-conformance && cargo test        # skips without pnpm
//! ```

pub mod harness;
