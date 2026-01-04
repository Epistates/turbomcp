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

/// Client capabilities per MCP 2025-11-25 specification
///
/// ## Version Support
/// - MCP 2025-11-25: roots, sampling, elicitation, experimental
/// - MCP 2025-11-25 draft (SEP-1686): + tasks
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

    /// Present if the client supports the Tasks API (MCP 2025-11-25 draft, SEP-1686)
    ///
    /// When present, indicates the client can act as a receiver for task-augmented requests
    /// from the server (e.g., sampling/createMessage, elicitation/create).
    #[cfg(feature = "mcp-tasks")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tasks: Option<ClientTasksCapabilities>,
}

/// Server capabilities per MCP 2025-11-25 specification
///
/// ## Version Support
/// - MCP 2025-11-25: logging, completions, prompts, resources, tools, experimental
/// - MCP 2025-11-25 draft (SEP-1686): + tasks
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

    /// Present if the server supports the Tasks API (MCP 2025-11-25 draft, SEP-1686)
    ///
    /// When present, indicates the server can act as a receiver for task-augmented requests
    /// from the client (e.g., tools/call).
    #[cfg(feature = "mcp-tasks")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tasks: Option<ServerTasksCapabilities>,
}

/// Sampling capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SamplingCapabilities {}

/// Elicitation capabilities per MCP 2025-11-25 specification
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ElicitationCapabilities {
    /// Whether the client performs JSON schema validation on elicitation responses
    /// If true, the client validates user input against the provided schema before sending
    #[serde(rename = "schemaValidation", skip_serializing_if = "Option::is_none")]
    pub schema_validation: Option<bool>,
}

impl ElicitationCapabilities {
    /// Create elicitation capabilities with schema validation enabled
    pub fn with_schema_validation(mut self) -> Self {
        self.schema_validation = Some(true);
        self
    }

    /// Create elicitation capabilities with schema validation disabled
    pub fn without_schema_validation(mut self) -> Self {
        self.schema_validation = Some(false);
        self
    }
}

/// Completion capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CompletionCapabilities {}

/// Roots capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RootsCapabilities {
    /// Whether list can change
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Logging capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LoggingCapabilities {}

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

// ========== Tasks API Capabilities (MCP 2025-11-25 draft, SEP-1686) ==========

/// Server tasks capabilities (MCP 2025-11-25 draft, SEP-1686)
///
/// Indicates which task operations and request types the server supports.
///
/// ## Example
///
/// ```rust,ignore
/// use turbomcp_protocol::types::{
///     ServerTasksCapabilities, TasksRequestsCapabilities, TasksToolsCapabilities
/// };
///
/// let tasks_caps = ServerTasksCapabilities {
///     list: Some(TasksListCapabilities {}),
///     cancel: Some(TasksCancelCapabilities {}),
///     requests: Some(TasksRequestsCapabilities {
///         tools: Some(TasksToolsCapabilities {
///             call: Some(TasksToolsCallCapabilities {}),
///         }),
///         ..Default::default()
///     }),
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg(feature = "mcp-tasks")]
pub struct ServerTasksCapabilities {
    /// Present if the server supports tasks/list
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list: Option<TasksListCapabilities>,

    /// Present if the server supports tasks/cancel
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancel: Option<TasksCancelCapabilities>,

    /// Present if the server supports task-augmented requests
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requests: Option<ServerTasksRequestsCapabilities>,
}

/// Client tasks capabilities (MCP 2025-11-25 draft, SEP-1686)
///
/// Indicates which task operations and request types the client supports.
///
/// ## Example
///
/// ```rust,ignore
/// use turbomcp_protocol::types::{
///     ClientTasksCapabilities, ClientTasksRequestsCapabilities,
///     TasksSamplingCapabilities, TasksSamplingCreateMessageCapabilities
/// };
///
/// let tasks_caps = ClientTasksCapabilities {
///     list: Some(TasksListCapabilities {}),
///     cancel: Some(TasksCancelCapabilities {}),
///     requests: Some(ClientTasksRequestsCapabilities {
///         sampling: Some(TasksSamplingCapabilities {
///             create_message: Some(TasksSamplingCreateMessageCapabilities {}),
///         }),
///         elicitation: Some(TasksElicitationCapabilities {
///             create: Some(TasksElicitationCreateCapabilities {}),
///         }),
///     }),
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg(feature = "mcp-tasks")]
pub struct ClientTasksCapabilities {
    /// Present if the client supports tasks/list
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list: Option<TasksListCapabilities>,

    /// Present if the client supports tasks/cancel
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancel: Option<TasksCancelCapabilities>,

    /// Present if the client supports task-augmented requests
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requests: Option<ClientTasksRequestsCapabilities>,
}

/// Task list capability
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg(feature = "mcp-tasks")]
pub struct TasksListCapabilities {}

/// Task cancel capability
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg(feature = "mcp-tasks")]
pub struct TasksCancelCapabilities {}

/// Server-side task-augmented requests capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg(feature = "mcp-tasks")]
pub struct ServerTasksRequestsCapabilities {
    /// Present if the server supports task-augmented tools/call
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<TasksToolsCapabilities>,
}

/// Client-side task-augmented requests capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg(feature = "mcp-tasks")]
pub struct ClientTasksRequestsCapabilities {
    /// Present if the client supports task-augmented sampling/createMessage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<TasksSamplingCapabilities>,

    /// Present if the client supports task-augmented elicitation/create
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<TasksElicitationCapabilities>,
}

/// Tools task capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg(feature = "mcp-tasks")]
pub struct TasksToolsCapabilities {
    /// Present if task-augmented tools/call is supported
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call: Option<TasksToolsCallCapabilities>,
}

/// Tools call task capability
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg(feature = "mcp-tasks")]
pub struct TasksToolsCallCapabilities {}

/// Sampling task capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg(feature = "mcp-tasks")]
pub struct TasksSamplingCapabilities {
    /// Present if task-augmented sampling/createMessage is supported
    #[serde(rename = "createMessage", skip_serializing_if = "Option::is_none")]
    pub create_message: Option<TasksSamplingCreateMessageCapabilities>,
}

/// Sampling create message task capability
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg(feature = "mcp-tasks")]
pub struct TasksSamplingCreateMessageCapabilities {}

/// Elicitation task capabilities
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg(feature = "mcp-tasks")]
pub struct TasksElicitationCapabilities {
    /// Present if task-augmented elicitation/create is supported
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create: Option<TasksElicitationCreateCapabilities>,
}

/// Elicitation create task capability
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg(feature = "mcp-tasks")]
pub struct TasksElicitationCreateCapabilities {}