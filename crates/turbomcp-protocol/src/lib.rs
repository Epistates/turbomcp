//! # turbomcp-protocol
//!
//! Per-spec-version typed MCP wire modules. Each version's `types` module is
//! **@generated** by `turbomcp-codegen` from the official schema and checked
//! in (reviewed on each spec update); behaviour (dispatch, capability
//! negotiation) is hand-written.
//!
//! - [`v2025_06_18`] — stateful model; `2025-11-25` without Tasks, `icons`
//!   or URL elicitation. Its conversions step down from the `2025-11-25` wire.
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
//! ## What a decode and re-encode keeps
//!
//! - **`{}` vs absent** is kept for the open objects that mean something by
//!   being there (capability markers such as `elicitation.url`, an empty
//!   `structuredContent`): they decode as `Option<JsonObject>`.
//! - **`null` vs absent** is kept for fields that may hold any JSON value
//!   (`structuredContent`, `error.data`): a present `null` is
//!   `Some(Value::Null)`.
//! - **Empty arrays and maps collapse to absent.** An optional list
//!   (`annotations.audience`, `tools` on a sampling request, an accepted
//!   elicitation's `content`) decodes to an empty `Vec`/map and is not
//!   written back. The two mostly mean the same thing, but not always (an
//!   empty `audience` is not plainly "no audience stated"), so something that
//!   must re-send a peer's message exactly should forward the JSON, not the
//!   typed value.
//! - **Unknown keys** survive on every result, tool, resource, resource
//!   template, prompt and implementation (`schema.ts` declares results open),
//!   in `_meta`, and in embedded JSON Schemas: the neutral types carry them
//!   as `extra`, and a key the target revision declares is never written from
//!   there. Inside other objects (a content block, an annotation) they are
//!   dropped.
//! - **Numbers** decode the same with `serde_json/arbitrary_precision` on.
//!
//! `no_std + alloc`: the generated types use only `core`/`alloc` paths (the
//! codegen remaps typify's `::std::` output), so this crate is wasm-portable.
#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

extern crate alloc;

use turbomcp_core as _;

mod lenient;
pub mod methods;
pub mod neutral;
pub mod server_card;
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
