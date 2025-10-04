//! LLM sampling types (MCP 2025-06-18)
//!
//! This module contains types for server-initiated LLM sampling,
//! allowing servers to request LLM interactions from clients.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{content::Content, core::Role};

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
    /// Maximum number of tokens to generate
    #[serde(rename = "maxTokens", skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Stop sequences
    #[serde(rename = "stopSequences", skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// Optional metadata per MCP 2025-06-18 specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// Model hint for selection (MCP 2025-06-18 compliant)
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

/// Model preferences for sampling (MCP 2025-06-18 compliant)
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

// DEPRECATED: Legacy tier-based enums kept for backward compatibility
// These are NOT part of MCP 2025-06-18 spec
/// **DEPRECATED** - Cost tier (legacy). Use [`ModelPreferences`] with `cost_priority` instead.
#[deprecated(
    since = "2.0.0",
    note = "Use ModelPreferences with priority values instead"
)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CostTier {
    /// Low cost priority
    Low,
    /// Medium cost priority
    Medium,
    /// High cost priority
    High,
}

/// **DEPRECATED** - Speed tier (legacy). Use [`ModelPreferences`] with `speed_priority` instead.
#[deprecated(
    since = "2.0.0",
    note = "Use ModelPreferences with priority values instead"
)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SpeedTier {
    /// Low speed priority
    Low,
    /// Medium speed priority
    Medium,
    /// High speed priority
    High,
}

/// **DEPRECATED** - Intelligence tier (legacy). Use [`ModelPreferences`] with `intelligence_priority` instead.
#[deprecated(
    since = "2.0.0",
    note = "Use ModelPreferences with priority values instead"
)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum IntelligenceTier {
    /// Low intelligence priority
    Low,
    /// Medium intelligence priority
    Medium,
    /// High intelligence priority
    High,
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
    #[serde(rename = "stopReason", skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    /// Optional metadata per MCP 2025-06-18 specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// Stop reason for generation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
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
