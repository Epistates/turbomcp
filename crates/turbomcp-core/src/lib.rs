//! # turbomcp-core
//!
//! Foundation layer for TurboMCP v4: the cross-version-stable types every other
//! crate builds on. `no_std + alloc` (compiles on `wasm32`), with `std`-gated
//! ergonomics (awaitable cancellation, std error integration).
//!
//! ## What lives here
//!
//! - [`ProtocolVersion`] — the single version representation (wire string
//!   `"2025-11-25"` … `"2026-07-28"`).
//! - [`JsonRpcMessage`] and its envelope ([`JsonRpcRequest`],
//!   [`JsonRpcResponse`], [`JsonRpcNotification`], [`JsonRpcError`],
//!   [`RequestId`]). No batches.
//! - [`McpError`] / [`McpResult`] — the unified error type and its JSON-RPC ↔
//!   HTTP mapping.
//! - [`Identity`] — who made the request (redaction-aware `Debug`).
//! - [`RequestContext`] — read-only request metadata + [`Extensions`] type-map.
//! - [`CancellationToken`] — always-present per-request cancellation.
//! - [`meta`] — `_meta` well-known keys and propagation policy.
//!
//! Per-version *semantic* wire types (`CallToolRequest`, …) are codegenned into
//! `turbomcp-protocol`, not here.
#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

extern crate alloc;

mod cancellation;
mod context;
mod error;
mod identity;
mod jsonrpc;
pub mod meta;
mod protocol_version;

pub use cancellation::CancellationToken;
pub use context::{Extensions, Implementation, LogLevel, RequestContext, TraceContext};
pub use error::{McpError, McpResult, codes};
pub use identity::{Claims, Identity, IdentityClaims, RedactedSubject};
pub use jsonrpc::{
    JsonRpcError, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, RequestId,
};
pub use protocol_version::ProtocolVersion;
