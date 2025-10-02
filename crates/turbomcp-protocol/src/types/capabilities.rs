//! MCP capability negotiation types
//!
//! This module contains types for capability discovery and negotiation between
//! MCP clients and servers. Capabilities define what features each side supports
//! and are exchanged during the initialization handshake.
//!
//! # Capability Types
//!
//! - [`ClientCapabilities`] - Client-side capabilities
//! - [`ServerCapabilities`] - Server-side capabilities
//! - Feature-specific capability structures for each MCP feature

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Client capabilities per MCP 2025-06-18 specification
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClientCapabilities {
    /// Experimental, non-standard capabilities that the client supports
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<HashMap<String, serde_json::Value>>,

    /// Present if the client supports listing roots
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roots: Option<RootsCapabilities>,

    /// Present if the client supports sampling from an LLM
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<SamplingCapabilities>,

    /// Present if the client supports elicitation from the server
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<ElicitationCapabilities>,
}

/// Server capabilities per MCP 2025-06-18 specification
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerCapabilities {
    /// Experimental, non-standard capabilities that the server supports
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<HashMap<String, serde_json::Value>>,

    /// Present if the server supports sending log messages to the client
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging: Option<LoggingCapabilities>,

    /// Present if the server supports argument autocompletion suggestions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completions: Option<CompletionCapabilities>,

    /// Present if the server offers any prompt templates
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<PromptsCapabilities>,

    /// Present if the server offers any resources to read
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcesCapabilities>,

    /// Present if the server offers any tools to call
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapabilities>,
}

/// Sampling capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SamplingCapabilities;

/// Elicitation capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ElicitationCapabilities;

/// Completion capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CompletionCapabilities;

/// Roots capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RootsCapabilities {
    /// Whether list can change
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Logging capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LoggingCapabilities;

/// Prompts capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PromptsCapabilities {
    /// Whether list can change
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Resources capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourcesCapabilities {
    /// Whether subscribe is supported
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscribe: Option<bool>,

    /// Whether list can change
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Tools capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolsCapabilities {
    /// Whether list can change
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}
