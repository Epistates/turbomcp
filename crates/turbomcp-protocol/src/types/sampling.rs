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

/// Model preferences for sampling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPreferences {
    /// Preferred model hints (not binding)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hints: Option<Vec<String>>,
    /// Cost tier preference
    #[serde(rename = "costTier", skip_serializing_if = "Option::is_none")]
    pub cost_tier: Option<CostTier>,
    /// Speed tier preference
    #[serde(rename = "speedTier", skip_serializing_if = "Option::is_none")]
    pub speed_tier: Option<SpeedTier>,
    /// Intelligence tier preference
    #[serde(rename = "intelligenceTier", skip_serializing_if = "Option::is_none")]
    pub intelligence_tier: Option<IntelligenceTier>,
}

/// Cost tier preferences
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CostTier {
    /// Low cost preference
    Low,
    /// Medium cost preference
    Medium,
    /// High cost preference (premium models)
    High,
}

/// Speed tier preferences
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SpeedTier {
    /// Low speed (high latency acceptable)
    Low,
    /// Medium speed
    Medium,
    /// High speed (low latency required)
    High,
}

/// Intelligence tier preferences
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum IntelligenceTier {
    /// Basic intelligence
    Low,
    /// Moderate intelligence
    Medium,
    /// High intelligence (most capable models)
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
