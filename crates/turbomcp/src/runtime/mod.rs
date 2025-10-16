//! Runtime support for full MCP 2025-06-18 protocol
//!
//! ## Architectural Decision (2024-10-14)
//!
//! All bidirectional transport runtime implementations now live in
//! `turbomcp-server/src/runtime/` as the **single source of truth**.
//!
//! The `#[server]` macro generates code that uses `ServerBuilder`'s transport
//! methods (`run_stdio()`, `run_http_with_config()`, `run_websocket_with_config()`),
//! ensuring consistent MCP 2025-06-18 protocol compliance across all patterns.
//!
//! ## Why This Module Is Now Empty
//!
//! Previously, this module contained duplicate implementations of:
//! - `stdio_bidirectional.rs` (484 lines) - DELETED
//! - `http_bidirectional.rs` (19KB) - DELETED
//! - `websocket_server.rs` (726 lines) - DELETED
//! - `websocket_bidirectional.rs` (290 lines) - DELETED (orphaned adapter)
//!
//! These duplicates caused:
//! - MCP protocol compliance drift
//! - Bug duplication (e.g., HTTP session ID bug)
//! - Zero test coverage for ServerBuilder pattern
//! - ~2,500 lines of redundant code
//!
//! ## Current Architecture
//!
//! ```text
//! #[server] macro (turbomcp-macros)
//!   ↓ generates run_stdio()/run_http()/run_websocket()
//! create_server() (turbomcp)
//!   ↓ builds
//! ServerBuilder (turbomcp-server)
//!   ↓ uses
//! turbomcp-server/src/runtime/* (SINGLE SOURCE OF TRUTH)
//!   - stdio.rs
//!   - http.rs
//!   - websocket.rs
//! ```
//!
//! All transport functionality is accessed through `ServerBuilder`, ensuring
//! that both usage patterns (macro and builder) share identical implementation.
