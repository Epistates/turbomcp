//! LLM sampling types
//!
//! This module contains types for server-initiated LLM sampling:
//! - MCP 2025-11-25: Basic text-based sampling
//! - MCP 2025-11-25 draft (SEP-1577): + Tool calling support

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{content::Content, core::Role};

#[cfg(feature = "mcp-sampling-tools")]
use super::tools::Tool;

/// Include context options for sampling
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum IncludeContext {
    /// No context
    None,
    /// This server only
    ThisServer,
    /// All servers
    AllServers,
}

/// Sampling message structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplingMessage {
    /// Message role
    pub role: Role,
    /// Message content
    pub content: Content,
    /// Optional message metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

/// Create message request (for LLM sampling)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMessageRequest {
    /// Messages to include in the sampling request
    pub messages: Vec<SamplingMessage>,
    /// Model preferences (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_preferences: Option<ModelPreferences>,
    /// System prompt (optional)
    #[serde(rename = "systemPrompt", skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// Include context from other servers
    #[serde(rename = "includeContext", skip_serializing_if = "Option::is_none")]
    pub include_context: Option<IncludeContext>,
    /// Temperature for sampling (0.0 to 2.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Maximum number of tokens to generate (required by MCP spec)
    #[serde(rename = "maxTokens")]
    pub max_tokens: u32,
    /// Stop sequences
    #[serde(rename = "stopSequences", skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// Tools that the model may use during generation (MCP 2025-11-25 draft, SEP-1577)
    /// The client MUST return an error if this field is provided but
    /// ClientCapabilities.sampling.tools is not declared
    #[cfg(feature = "mcp-sampling-tools")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    /// Controls how the model uses tools (MCP 2025-11-25 draft, SEP-1577)
    /// The client MUST return an error if this field is provided but
    /// ClientCapabilities.sampling.tools is not declared
    /// Default is `{ mode: "auto" }`
    #[cfg(feature = "mcp-sampling-tools")]
    #[serde(rename = "toolChoice", skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    /// Task metadata for task-augmented sampling (MCP 2025-11-25 draft, SEP-1686)
    ///
    /// When present, indicates the client should execute this sampling request as a long-running
    /// task and return a CreateTaskResult instead of the immediate CreateMessageResult.
    /// The actual result can be retrieved later via tasks/result.
    #[cfg(feature = "mcp-tasks")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<crate::types::tasks::TaskMetadata>,
    /// Optional metadata per MCP 2025-11-25 specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// Model hint for selection (MCP 2025-11-25 compliant)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelHint {
    /// Model name hint (substring matching)
    /// Examples: "claude-3-5-sonnet", "sonnet", "claude"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl ModelHint {
    /// Create a new model hint with a name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
        }
    }
}

/// Model preferences for sampling (MCP 2025-11-25 compliant)
///
/// The spec changed from tier-based to priority-based system.
/// Priorities are 0.0-1.0 where 0 = not important, 1 = most important.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPreferences {
    /// Optional hints for model selection (evaluated in order)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hints: Option<Vec<ModelHint>>,

    /// Cost priority (0.0 = not important, 1.0 = most important)
    #[serde(rename = "costPriority", skip_serializing_if = "Option::is_none")]
    pub cost_priority: Option<f64>,

    /// Speed priority (0.0 = not important, 1.0 = most important)
    #[serde(rename = "speedPriority", skip_serializing_if = "Option::is_none")]
    pub speed_priority: Option<f64>,

    /// Intelligence priority (0.0 = not important, 1.0 = most important)
    #[serde(
        rename = "intelligencePriority",
        skip_serializing_if = "Option::is_none"
    )]
    pub intelligence_priority: Option<f64>,
}

/// Create message result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMessageResult {
    /// The role of the message (required by MCP specification)
    pub role: super::core::Role,
    /// The generated message content
    pub content: Content,
    /// Model used for generation (required by MCP specification)
    pub model: String,
    /// Stop reason (if applicable)
    ///
    /// Uses the StopReason enum with camelCase serialization for MCP 2025-11-25 compliance.
    #[serde(rename = "stopReason", skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
    /// Optional metadata per MCP 2025-11-25 specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// Stop reason for generation
///
/// Per MCP 2025-11-25 spec, these values use camelCase serialization for interoperability.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum StopReason {
    /// Generation completed naturally
    EndTurn,
    /// Hit maximum token limit
    MaxTokens,
    /// Hit a stop sequence
    StopSequence,
    /// Content filtering triggered
    ContentFilter,
    /// Tool use required
    ToolUse,
}

/// Usage statistics for sampling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageStats {
    /// Input tokens consumed
    #[serde(rename = "inputTokens", skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u32>,
    /// Output tokens generated
    #[serde(rename = "outputTokens", skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u32>,
    /// Total tokens used
    #[serde(rename = "totalTokens", skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u32>,
}

/// Tool choice mode (MCP 2025-11-25 draft, SEP-1577)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "lowercase")]
#[cfg(feature = "mcp-sampling-tools")]
pub enum ToolChoiceMode {
    /// Model decides whether to use tools (default)
    #[default]
    Auto,
    /// Model MUST use at least one tool before completing
    Required,
    /// Model MUST NOT use any tools
    None,
}

/// Controls tool selection behavior for sampling requests (MCP 2025-11-25 draft, SEP-1577)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg(feature = "mcp-sampling-tools")]
pub struct ToolChoice {
    /// Controls the tool use ability of the model
    /// - "auto": Model decides whether to use tools (default)
    /// - "required": Model MUST use at least one tool before completing
    /// - "none": Model MUST NOT use any tools
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<ToolChoiceMode>,
}

#[cfg(feature = "mcp-sampling-tools")]
impl ToolChoice {
    /// Create a new ToolChoice with auto mode
    pub fn auto() -> Self {
        Self {
            mode: Some(ToolChoiceMode::Auto),
        }
    }

    /// Create a new ToolChoice requiring tool use
    pub fn required() -> Self {
        Self {
            mode: Some(ToolChoiceMode::Required),
        }
    }

    /// Create a new ToolChoice forbidding tool use
    pub fn none() -> Self {
        Self {
            mode: Some(ToolChoiceMode::None),
        }
    }
}

#[cfg(feature = "mcp-sampling-tools")]
impl Default for ToolChoice {
    fn default() -> Self {
        Self::auto()
    }
}

#[cfg(test)]
#[cfg(feature = "mcp-sampling-tools")]
mod tests {
    use super::*;

    #[test]
    fn test_tool_choice_mode_serialization() {
        assert_eq!(
            serde_json::to_string(&ToolChoiceMode::Auto).unwrap(),
            "\"auto\""
        );
        assert_eq!(
            serde_json::to_string(&ToolChoiceMode::Required).unwrap(),
            "\"required\""
        );
        assert_eq!(
            serde_json::to_string(&ToolChoiceMode::None).unwrap(),
            "\"none\""
        );
    }

    #[test]
    fn test_tool_choice_constructors() {
        let auto = ToolChoice::auto();
        assert_eq!(auto.mode, Some(ToolChoiceMode::Auto));

        let required = ToolChoice::required();
        assert_eq!(required.mode, Some(ToolChoiceMode::Required));

        let none = ToolChoice::none();
        assert_eq!(none.mode, Some(ToolChoiceMode::None));
    }

    #[test]
    fn test_tool_choice_default() {
        let default = ToolChoice::default();
        assert_eq!(default.mode, Some(ToolChoiceMode::Auto));
    }

    #[test]
    fn test_tool_choice_serialization() {
        let choice = ToolChoice::required();
        let json = serde_json::to_string(&choice).unwrap();
        assert!(json.contains("\"mode\":\"required\""));
    }
}
