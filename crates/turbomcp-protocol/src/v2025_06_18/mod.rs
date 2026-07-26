//! MCP `2025-06-18` — the previous stable revision, still widely deployed.
//!
//! Wire string `"2025-06-18"`. Same stateful model as [`2025-11-25`]: an
//! `initialize` handshake, `ping`, `resources/subscribe`/`unsubscribe`, and
//! per-session negotiated state. What it does **not** have is everything
//! `2025-11-25` added — Tasks (`tasks/*`, `Tool.execution`,
//! `ServerCapabilities.tasks`), `icons` on tools/resources/templates/prompts/
//! resource links, URL-mode elicitation, and the `description`/`websiteUrl`
//! fields on `Implementation`. Its method set is otherwise identical.
//!
//! The [`types`] module is `@generated` by `turbomcp-codegen` — do not edit.
//! [`convert`] holds the hand-written bridges to and from the `2025-11-25`
//! wire; see that module for why the conversions are expressed that way rather
//! than duplicated from the neutral types.
//!
//! [`2025-11-25`]: crate::v2025_11_25

pub mod convert;
pub mod types;
