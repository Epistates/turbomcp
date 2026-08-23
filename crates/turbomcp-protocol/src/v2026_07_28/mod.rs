//! MCP `2026-07-28` — the stateless protocol model.
//!
//! Frozen on 2026-07-28 and generated from the dated `schema/2026-07-28/`.
//! This module was called `draft` while the revision was in development;
//! `turbomcp_protocol::draft` remains as a deprecated alias.
//! Stateless: per-request `_meta` version, `server/discover`,
//! `subscriptions/listen`, MRTR (`InputRequiredResult`). Tasks is delivered via
//! the `extensions` capability rather than core methods. The [`types`] module
//! is `@generated` by `turbomcp-codegen` — do not edit.

pub mod types;
