//! Types for the MCP connection initialization and handshake process.
//!
//! Canonical definitions live in [`turbomcp_types::wire`]; this module
//! re-exports them so existing `turbomcp_protocol::types::initialization::*`
//! imports continue to work.

pub use turbomcp_types::{InitializeRequest, InitializeResult, InitializedNotification};
