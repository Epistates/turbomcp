//! # turbomcp-protocol
//!
//! Per-spec-version typed MCP wire modules. Each version's `types` module is
//! **@generated** by `turbomcp-codegen` from the official schema and checked
//! in (reviewed on each spec update); behaviour (dispatch, capability
//! negotiation) is hand-written.
//!
//! - [`v2025_11_25`] — stateful model: `initialize`, `ping`, `resources/subscribe`,
//!   and **core Tasks** (`tasks/get|list|cancel|result`).
//! - [`draft`] — `2026-07-28`: stateless `server/discover`,
//!   `subscriptions/listen`, MRTR (`InputRequiredResult`); Tasks moves to the
//!   `extensions` mechanism.
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

pub mod draft;
pub mod methods;
pub mod neutral;
pub mod v2025_11_25;
pub mod version;
