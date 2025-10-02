//! Argument autocompletion types
//!
//! This module contains types for the MCP argument completion system,
//! allowing servers to provide completion suggestions for tool and prompt arguments.

use serde::{Deserialize, Serialize};

/// Argument information for completion
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArgumentInfo {
    /// The name of the argument being completed
    pub name: String,
    /// The current value of the argument (may be partial)
    pub value: String,
}

/// Reference to a prompt for completion
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PromptReference {
    /// Reference type (always "ref/prompt")
    #[serde(rename = "type")]
    pub ref_type: String,
    /// The name of the prompt
    pub name: String,
    /// Human-readable title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl PromptReference {
    /// Create a new prompt reference
    pub fn new<N: Into<String>>(name: N) -> Self {
        Self {
            ref_type: "ref/prompt".to_string(),
            name: name.into(),
            title: None,
        }
    }

    /// Create a new prompt reference with title
    pub fn with_title<N: Into<String>, T: Into<String>>(name: N, title: T) -> Self {
        Self {
            ref_type: "ref/prompt".to_string(),
            name: name.into(),
            title: Some(title.into()),
        }
    }
}

/// Data for prompt reference (excluding the type field)
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PromptReferenceData {
    /// The name of the prompt
    pub name: String,
    /// Human-readable title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Data for resource template reference (excluding the type field)
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ResourceTemplateReferenceData {
    /// The URI or URI template of the resource
    pub uri: String,
}

/// Reference types for completion
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum CompletionReference {
    /// Reference to a prompt
    #[serde(rename = "ref/prompt")]
    Prompt(PromptReferenceData),
    /// Reference to a resource template
    #[serde(rename = "ref/resource")]
    ResourceTemplate(ResourceTemplateReferenceData),
}

/// Additional context for completions
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct CompletionContext {
    /// Previously-resolved variables in a URI template or prompt
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<std::collections::HashMap<String, String>>,
}

/// Parameters for completion/complete request
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct CompleteRequestParams {
    /// The argument's information
    pub argument: ArgumentInfo,
    /// Reference to the item being completed
    #[serde(rename = "ref")]
    pub reference: CompletionReference,
    /// Additional, optional context for completions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<CompletionContext>,
}

/// Completion option/suggestion
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompletionOption {
    /// The completion value
    pub value: String,
    /// Human-readable label (optional, falls back to value)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Type of completion (file, directory, function, etc.)
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub completion_type: Option<String>,
    /// Documentation for this completion
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
    /// Sort priority (lower numbers appear first)
    #[serde(rename = "sortPriority", skip_serializing_if = "Option::is_none")]
    pub sort_priority: Option<u32>,
    /// Text to insert (if different from value)
    #[serde(rename = "insertText", skip_serializing_if = "Option::is_none")]
    pub insert_text: Option<String>,
}

/// Completion data structure per MCP specification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompletionData {
    /// An array of completion values. Must not exceed 100 items.
    pub values: Vec<String>,
    /// The total number of completion options available. This can exceed the number of values actually sent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u32>,
    /// Indicates whether there are additional completion options beyond those provided
    #[serde(rename = "hasMore", skip_serializing_if = "Option::is_none")]
    pub has_more: Option<bool>,
}

/// Completion response
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompletionResponse {
    /// Completion data per MCP 2025-06-18 specification
    pub completion: CompletionData,
    /// Optional metadata per MCP 2025-06-18 specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// Server's response to a completion/complete request per MCP 2025-06-18 specification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompleteResult {
    /// Completion data
    pub completion: CompletionData,
    /// Optional metadata per MCP 2025-06-18 specification
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

impl CompleteResult {
    /// Create a new completion result
    pub fn new(completion: CompletionData) -> Self {
        Self {
            completion,
            _meta: None,
        }
    }

    /// Create a completion result with values
    pub fn with_values(values: Vec<String>) -> Self {
        Self::new(CompletionData {
            values,
            total: None,
            has_more: None,
        })
    }

    /// Create a completion result with values and metadata
    pub fn with_values_and_total(values: Vec<String>, total: u32, has_more: bool) -> Self {
        Self::new(CompletionData {
            values,
            total: Some(total),
            has_more: Some(has_more),
        })
    }

    /// Add metadata to this result
    pub fn with_meta(mut self, meta: serde_json::Value) -> Self {
        self._meta = Some(meta);
        self
    }
}
