//! # turbomcp-protocol
//!
//! The cross-version [`neutral`] types that handlers speak, and the routing
//! primitives ([`methods`], [`version`]) that pick a revision per request.
//! `neutral` is the stable contract.
//!
//! Under it sit three per-revision wire modules, `v2025_06_18`, `v2025_11_25`
//! and `v2026_07_28`. Each one's `types` is **@generated** by
//! `turbomcp-codegen` from that revision's official schema. They are public
//! because the other TurboMCP crates convert through them, but they are
//! hidden from the docs and are not covered by semver: an upstream erratum or
//! a codegen fix can change them in any release. Convert to and from them
//! through the `From` impls on the `neutral` types.
//!
//! - `v2025_06_18`: stateful; `2025-11-25` without Tasks, `icons` or URL
//!   elicitation. Its conversions step down from the `2025-11-25` wire.
//! - `v2025_11_25`: stateful: `initialize`, `ping`, `resources/subscribe`, and
//!   core Tasks (`tasks/get|list|cancel|result`).
//! - `v2026_07_28`: stateless: `server/discover`, `subscriptions/listen`, MRTR
//!   (`InputRequiredResult`); Tasks moves to the `extensions` mechanism.
//!
//! The `VersionDispatcher` that consumes these is in `turbomcp-server`: it is
//! generic over the user's `McpServerCore`, which sits above this layer.
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
// The generated wire modules: public for the other TurboMCP crates, hidden
// and outside semver (see the crate docs).
#[doc(hidden)]
pub mod v2025_06_18;
#[doc(hidden)]
pub mod v2025_11_25;
#[doc(hidden)]
pub mod v2026_07_28;
pub mod version;
