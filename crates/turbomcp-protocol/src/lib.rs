//! # turbomcp-protocol
//!
//! Per-spec-version typed MCP wire modules. Each version's `types` module is
//! **@generated** by `turbomcp-codegen` from the official schema and checked
//! in (reviewed on each spec update); behaviour (dispatch, capability
//! negotiation) is hand-written.
//!
//! - [`v2025_11_25`] — stateful model: `initialize`, `ping`, `resources/subscribe`,
//!   and **core Tasks** (`tasks/get|list|cancel|result`).
//! - [`v2026_07_28`] — stateless model: `server/discover`,
//!   `subscriptions/listen`, MRTR (`InputRequiredResult`); Tasks moves to the
//!   `extensions` mechanism. (Named `draft` until the spec froze on
//!   2026-07-28; [`draft`] survives as a deprecated alias.)
//!
//! The cross-version [`neutral`] handler surface and the [`methods`]/[`version`]
//! routing primitives live here; the `VersionDispatcher` that consumes them is
//! in `turbomcp-server` (it is generic over the user's `McpServerCore`, which
//! sits above this layer — keeping the dependency graph acyclic).
//!
//! `no_std + alloc`: the generated types use only `core`/`alloc` paths (the
//! codegen remaps typify's `::std::` output), so this crate is wasm-portable.
#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

extern crate alloc;

use turbomcp_core as _;

pub mod methods;
pub mod neutral;
pub mod v2025_06_18;
pub mod v2025_11_25;
pub mod v2026_07_28;
pub mod version;

/// The `2026-07-28` wire module under its pre-freeze name.
///
/// It was called `draft` while the revision was still in development. Kept as
/// a deprecated re-export so `turbomcp_protocol::draft::types` keeps resolving;
/// use [`v2026_07_28`] instead. A future draft will get its own `draft` module
/// rather than reusing this alias.
#[deprecated(since = "4.0.0-alpha.2", note = "the draft froze: use v2026_07_28")]
pub use v2026_07_28 as draft;
