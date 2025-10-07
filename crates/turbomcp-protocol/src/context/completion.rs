//! Completion context types for autocompletion and suggestion systems.
//!
//! This module contains types for handling completion requests across various
//! MCP contexts including prompts, resource templates, and tools.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Context for completion/autocompletion requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionContext {
    /// Unique completion request ID
    pub completion_id: String,
    /// Reference being completed (prompt, resource template, etc.)
    pub completion_ref: CompletionReference,
    /// Current argument being completed
    pub argument_name: Option<String>,
    /// Partial value being completed
    pub partial_value: Option<String>,
    /// Previously resolved arguments
    pub resolved_arguments: HashMap<String, String>,
    /// Available completion options
    pub completions: Vec<CompletionOption>,
    /// Cursor position for completion
    pub cursor_position: Option<usize>,
    /// Maximum number of completions to return
    pub max_completions: Option<usize>,
    /// Whether more completions are available
    pub has_more: bool,
    /// Total number of available completions
    pub total_completions: Option<usize>,
    /// Client capabilities for completion
    pub client_capabilities: Option<CompletionCapabilities>,
    /// Completion metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Client capabilities for completion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionCapabilities {
    /// Supports paginated completions
    pub supports_pagination: bool,
    /// Supports fuzzy matching
    pub supports_fuzzy: bool,
    /// Maximum batch size
    pub max_batch_size: usize,
    /// Supports rich completion items with descriptions
    pub supports_descriptions: bool,
}

/// Reference type for completion context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompletionReference {
    /// Completing a prompt argument
    Prompt {
        /// Prompt name
        name: String,
        /// Argument being completed
        argument: String,
    },
    /// Completing a resource template parameter
    ResourceTemplate {
        /// Template name
        name: String,
        /// Parameter being completed
        parameter: String,
    },
    /// Completing a tool argument
    Tool {
        /// Tool name
        name: String,
        /// Argument being completed
        argument: String,
    },
    /// Custom completion context
    Custom {
        /// Custom reference type
        ref_type: String,
        /// Reference metadata
        metadata: HashMap<String, serde_json::Value>,
    },
}

/// Completion option with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionOption {
    /// Completion value
    pub value: String,
    /// Human-readable label
    pub label: Option<String>,
    /// Completion type (value, keyword, function, etc.)
    pub completion_type: Option<String>,
    /// Additional documentation
    pub documentation: Option<String>,
    /// Sort priority (lower = higher priority)
    pub sort_priority: Option<i32>,
    /// Whether this option requires additional input
    pub insert_text: Option<String>,
}

impl CompletionContext {
    /// Create a new completion context
    pub fn new(completion_ref: CompletionReference) -> Self {
        Self {
            completion_id: Uuid::new_v4().to_string(),
            completion_ref,
            argument_name: None,
            partial_value: None,
            resolved_arguments: HashMap::new(),
            completions: Vec::new(),
            cursor_position: None,
            max_completions: Some(100),
            has_more: false,
            total_completions: None,
            client_capabilities: None,
            metadata: HashMap::new(),
        }
    }

    /// Add a completion option
    pub fn add_completion(&mut self, option: CompletionOption) {
        self.completions.push(option);
    }

    /// Set resolved arguments
    pub fn with_resolved_arguments(mut self, args: HashMap<String, String>) -> Self {
        self.resolved_arguments = args;
        self
    }
}
