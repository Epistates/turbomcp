//! Filesystem boundaries types for the current MCP protocol.
//!
//! This module contains types for filesystem boundary discovery,
//! allowing servers to understand client filesystem access boundaries.

use serde::{Deserialize, Serialize};

// `Root` and `ListRootsResult` are defined in `turbomcp-types` so that
// `RequestContext::list_roots` can return them without inverting the crate
// layering. Re-exported here so `turbomcp_protocol::types::Root` still resolves.
pub use turbomcp_types::{ListRootsResult, Root, validate_root_uri};

/// List roots request with optional metadata
/// Note: Roots do not support pagination, only metadata
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListRootsRequest {
    /// Optional metadata per the current MCP specification
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// Roots list changed notification (no parameters)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootsListChangedNotification {}
