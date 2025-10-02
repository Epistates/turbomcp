//! Prompt template types
//!
//! This module contains types for the MCP prompt template system,
//! including prompt definitions, arguments, and prompt requests/responses.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    content::Content,
    core::{Cursor, Role},
};

/// Prompt definition per MCP 2025-06-18 specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt {
    /// Prompt name (programmatic identifier)
    pub name: String,

    /// Display title for UI contexts (optional, falls back to name if not provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// An optional description of what this prompt provides
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// A list of arguments to use for templating the prompt
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<PromptArgument>>,

    /// General metadata field for extensions and custom data
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, serde_json::Value>>,
}

/// Prompt argument definition per MCP 2025-06-18 specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptArgument {
    /// Argument name (programmatic identifier)
    pub name: String,

    /// Display title for UI contexts (optional, falls back to name if not provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// A human-readable description of the argument
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Whether this argument must be provided
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
}

/// Prompt input parameters
pub type PromptInput = HashMap<String, serde_json::Value>;

/// List prompts request with optional pagination
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListPromptsRequest {
    /// Optional cursor for pagination
    /// An opaque token representing the current pagination position.
    /// If provided, the server should return results starting after this cursor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    /// Optional metadata per MCP 2025-06-18 specification
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
    /// Optional metadata per MCP 2025-06-18 specification
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
    /// Optional metadata per MCP 2025-06-18 specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// Get prompt result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPromptResult {
    /// Prompt description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Prompt messages
    pub messages: Vec<PromptMessage>,
    /// Optional metadata per MCP 2025-06-18 specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// Prompt message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptMessage {
    /// Message role
    pub role: Role,
    /// Message content
    pub content: Content,
}
