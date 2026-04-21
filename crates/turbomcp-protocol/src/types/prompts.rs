//! Prompt template types.
//!
//! `Prompt`, `PromptArgument`, `PromptMessage`, and the `GetPromptResult` wire
//! wrapper are canonically defined in [`turbomcp_types`]. This module
//! re-exports them plus protocol-local list wrappers that reference
//! protocol's `Cursor`.
//!
//! Note: types' `PromptMessage.content` is `Content` (the 5-variant MCP
//! `ContentBlock` union); protocol's `ContentBlock` is an alias for the same
//! type, so wire format matches.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub use turbomcp_types::{GetPromptResult, Prompt, PromptArgument, PromptMessage};

use super::core::Cursor;

/// Prompt input parameters
pub type PromptInput = HashMap<String, serde_json::Value>;

/// List prompts request with optional pagination
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListPromptsRequest {
    /// Optional cursor for pagination.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    /// Optional metadata per the current MCP specification
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// List prompts result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPromptsResult {
    /// Available prompts
    pub prompts: Vec<Prompt>,
    /// Optional continuation token
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
    /// Optional metadata per the current MCP specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// Get prompt request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPromptRequest {
    /// Prompt name
    pub name: String,
    /// Prompt arguments
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<PromptInput>,
    /// Optional metadata per the current MCP specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}
