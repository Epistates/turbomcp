//! Filesystem boundaries types (MCP 2025-06-18)
//!
//! This module contains types for filesystem boundary discovery,
//! allowing servers to understand client filesystem access boundaries.

use serde::{Deserialize, Serialize};

use super::core::Uri;

/// Filesystem root definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Root {
    /// Root URI (typically a file:// URI)
    pub uri: Uri,
    /// Optional human-readable name for this root
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// List roots request with optional metadata
/// Note: Roots do not support pagination, only metadata
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListRootsRequest {
    /// Optional metadata per MCP 2025-06-18 specification
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// List roots result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListRootsResult {
    /// Available filesystem roots
    pub roots: Vec<Root>,
    /// Optional metadata per MCP 2025-06-18 specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// Roots list changed notification (no parameters)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootsListChangedNotification {}
