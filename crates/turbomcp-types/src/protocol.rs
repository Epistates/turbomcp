//! MCP Protocol types for Tasks, Elicitation, and Sampling (MCP 2025-11-25).
//!
//! This module provides the specialized types introduced in the MCP 2025-11-25
//! specification for advanced protocol features.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use crate::content::{Role, SamplingContent, SamplingContentBlock};
use crate::definitions::Tool;

// =============================================================================
// Tasks (SEP-1686)
// =============================================================================

/// Metadata for augmenting a request with task execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TaskMetadata {
    /// Requested duration in milliseconds to retain task from creation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<u64>,
}

/// Data associated with a task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Task {
    /// The task identifier.
    #[serde(rename = "taskId")]
    pub task_id: String,
    /// Current task state.
    pub status: TaskStatus,
    /// Optional human-readable message describing the current task state.
    #[serde(rename = "statusMessage", skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
    /// ISO 8601 timestamp when the task was created.
    #[serde(rename = "createdAt")]
    pub created_at: String,
    /// ISO 8601 timestamp when the task was last updated.
    #[serde(rename = "lastUpdatedAt")]
    pub last_updated_at: String,
    /// Actual retention duration from creation in milliseconds, null for unlimited.
    pub ttl: Option<u64>,
    /// Suggested polling interval in milliseconds.
    #[serde(rename = "pollInterval", skip_serializing_if = "Option::is_none")]
    pub poll_interval: Option<u64>,
}

/// The status of a task.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Task was cancelled.
    Cancelled,
    /// Task completed successfully.
    Completed,
    /// Task failed.
    Failed,
    /// Task requires additional input from the user.
    InputRequired,
    /// Task is currently running.
    Working,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => f.write_str("cancelled"),
            Self::Completed => f.write_str("completed"),
            Self::Failed => f.write_str("failed"),
            Self::InputRequired => f.write_str("input_required"),
            Self::Working => f.write_str("working"),
        }
    }
}

/// Result of a task-augmented request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CreateTaskResult {
    /// The created task.
    pub task: Task,
    /// Extension metadata.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

/// Result of a request to list tasks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ListTasksResult {
    /// List of tasks.
    pub tasks: Vec<Task>,
    /// Opaque token for pagination.
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// Extension metadata.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

/// Metadata for associating messages with a task.
///
/// Include in `_meta` under key `io.modelcontextprotocol/related-task`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RelatedTaskMetadata {
    /// The task identifier this message is associated with.
    #[serde(rename = "taskId")]
    pub task_id: String,
}

// =============================================================================
// Elicitation (SEP-1036)
// =============================================================================

/// Parameters for an elicitation request.
///
/// Per MCP 2025-11-25, `mode` is optional for form requests (defaults to `"form"`)
/// but required for URL requests. `Serialize` and `Deserialize` are implemented
/// manually to handle the optional `mode` tag on the form variant.
#[derive(Debug, Clone, PartialEq)]
pub enum ElicitRequestParams {
    /// Form elicitation (structured input)
    Form(ElicitRequestFormParams),
    /// URL elicitation (out-of-band interaction)
    Url(ElicitRequestURLParams),
}

impl Serialize for ElicitRequestParams {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Form(params) => {
                // Serialize form params with mode: "form"
                let mut value = serde_json::to_value(params).map_err(serde::ser::Error::custom)?;
                if let Some(obj) = value.as_object_mut() {
                    obj.insert("mode".into(), Value::String("form".into()));
                }
                value.serialize(serializer)
            }
            Self::Url(params) => {
                // Serialize URL params with mode: "url"
                let mut value = serde_json::to_value(params).map_err(serde::ser::Error::custom)?;
                if let Some(obj) = value.as_object_mut() {
                    obj.insert("mode".into(), Value::String("url".into()));
                }
                value.serialize(serializer)
            }
        }
    }
}

impl<'de> Deserialize<'de> for ElicitRequestParams {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let mode = value.get("mode").and_then(|v| v.as_str()).unwrap_or("form");

        match mode {
            "url" => {
                let params: ElicitRequestURLParams =
                    serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                Ok(Self::Url(params))
            }
            _ => {
                // Default to "form" when mode is absent or explicitly "form"
                let params: ElicitRequestFormParams =
                    serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                Ok(Self::Form(params))
            }
        }
    }
}

/// Parameters for form-based elicitation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ElicitRequestFormParams {
    /// Message to show the user.
    pub message: String,
    /// JSON Schema for the requested information.
    #[serde(rename = "requestedSchema")]
    pub requested_schema: Value,
    /// Task metadata if this is a task-augmented request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<TaskMetadata>,
    /// Extension metadata.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

/// Parameters for URL-based elicitation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ElicitRequestURLParams {
    /// Message to show the user.
    pub message: String,
    /// URL the user should navigate to.
    pub url: String,
    /// Unique elicitation ID.
    #[serde(rename = "elicitationId")]
    pub elicitation_id: String,
    /// Task metadata if this is a task-augmented request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<TaskMetadata>,
    /// Extension metadata.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

/// Result of an elicitation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ElicitResult {
    /// Action taken by the user.
    pub action: ElicitAction,
    /// Form content (only if action is "accept").
    /// Values are constrained to: string | number | boolean | string[]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    /// Extension metadata.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

/// Action taken in response to elicitation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ElicitAction {
    /// User accepted the request.
    Accept,
    /// User declined the request.
    Decline,
    /// User cancelled or dismissed the request.
    Cancel,
}

impl std::fmt::Display for ElicitAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Accept => f.write_str("accept"),
            Self::Decline => f.write_str("decline"),
            Self::Cancel => f.write_str("cancel"),
        }
    }
}

/// Notification that a URL elicitation has completed.
///
/// New in MCP 2025-11-25. Method: `notifications/elicitation/complete`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ElicitationCompleteNotification {
    /// The elicitation ID that completed.
    #[serde(rename = "elicitationId")]
    pub elicitation_id: String,
}

// =============================================================================
// Sampling (SEP-1577)
// =============================================================================

/// Parameters for a `sampling/createMessage` request.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CreateMessageRequest {
    /// Messages to include in the context.
    #[serde(default)]
    pub messages: Vec<SamplingMessage>,
    /// Max tokens to sample (required per spec, defaults to 0 for builder pattern).
    #[serde(rename = "maxTokens")]
    pub max_tokens: u32,
    /// Model selection preferences.
    #[serde(rename = "modelPreferences", skip_serializing_if = "Option::is_none")]
    pub model_preferences: Option<ModelPreferences>,
    /// Optional system prompt.
    #[serde(rename = "systemPrompt", skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// Context inclusion preference (soft-deprecated for thisServer/allServers).
    #[serde(rename = "includeContext", skip_serializing_if = "Option::is_none")]
    pub include_context: Option<IncludeContext>,
    /// Sampling temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Stop sequences.
    #[serde(rename = "stopSequences", skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// Task metadata if this is a task-augmented request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<TaskMetadata>,
    /// Available tools for the model (requires client `sampling.tools` capability).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    /// Tool usage constraints (requires client `sampling.tools` capability).
    #[serde(rename = "toolChoice", skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    /// Optional metadata to pass through to the LLM provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    /// Extension metadata.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

/// Message in a sampling request.
///
/// Per MCP 2025-11-25, `content` can be a single `SamplingMessageContentBlock`
/// or an array of them.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SamplingMessage {
    /// Message role.
    pub role: Role,
    /// Message content (single block or array per spec).
    pub content: SamplingContentBlock,
    /// Extension metadata.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

impl SamplingMessage {
    /// Create a user message with text content.
    #[must_use]
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: SamplingContent::text(text).into(),
            meta: None,
        }
    }

    /// Create an assistant message with text content.
    #[must_use]
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: SamplingContent::text(text).into(),
            meta: None,
        }
    }
}

/// Preferences for model selection.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ModelPreferences {
    /// Hints for selecting a model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hints: Option<Vec<ModelHint>>,
    /// Cost preference (0.0 to 1.0).
    #[serde(rename = "costPriority", skip_serializing_if = "Option::is_none")]
    pub cost_priority: Option<f64>,
    /// Speed preference (0.0 to 1.0).
    #[serde(rename = "speedPriority", skip_serializing_if = "Option::is_none")]
    pub speed_priority: Option<f64>,
    /// Intelligence preference (0.0 to 1.0).
    #[serde(
        rename = "intelligencePriority",
        skip_serializing_if = "Option::is_none"
    )]
    pub intelligence_priority: Option<f64>,
}

/// Hint for model selection.
///
/// Per spec, `name` is optional and treated as a substring match against model names.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ModelHint {
    /// Name pattern for model selection (substring match).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl std::fmt::Display for IncludeContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AllServers => f.write_str("allServers"),
            Self::ThisServer => f.write_str("thisServer"),
            Self::None => f.write_str("none"),
        }
    }
}

/// Context inclusion mode for sampling.
///
/// `thisServer` and `allServers` are soft-deprecated in 2025-11-25.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum IncludeContext {
    /// Include context from all servers (soft-deprecated).
    #[serde(rename = "allServers")]
    AllServers,
    /// Include context only from this server (soft-deprecated).
    #[serde(rename = "thisServer")]
    ThisServer,
    /// Do not include additional context.
    #[serde(rename = "none")]
    None,
}

/// Tool usage constraints for sampling.
///
/// Per spec, `mode` is optional and defaults to `"auto"`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ToolChoice {
    /// Controls the tool use ability of the model (defaults to auto).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<ToolChoiceMode>,
}

/// Mode for tool choice.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ToolChoiceMode {
    /// Model decides whether to use tools (default).
    Auto,
    /// Model MUST NOT use any tools.
    None,
    /// Model MUST use at least one tool.
    Required,
}

impl std::fmt::Display for ToolChoiceMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => f.write_str("auto"),
            Self::None => f.write_str("none"),
            Self::Required => f.write_str("required"),
        }
    }
}

/// Result of a sampling request.
///
/// Per spec, extends both `Result` and `SamplingMessage`, so it has
/// `role`, `content` (as SamplingContentBlock), `model`, and `stopReason`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CreateMessageResult {
    /// The role of the generated message.
    pub role: Role,
    /// The sampled content (single block or array per SamplingMessage).
    pub content: SamplingContentBlock,
    /// The name of the model that generated the message.
    pub model: String,
    /// The reason why sampling stopped, if known.
    #[serde(rename = "stopReason", skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    /// Extension metadata.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

// =============================================================================
// Capabilities
// =============================================================================

/// Capabilities supported by a client.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ClientCapabilities {
    /// Support for elicitation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<ElicitationCapabilities>,
    /// Support for sampling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<SamplingCapabilities>,
    /// Support for roots.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roots: Option<RootsCapabilities>,
    /// Support for tasks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tasks: Option<ClientTaskCapabilities>,
    /// Experimental capabilities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<HashMap<String, Value>>,
}

/// Elicitation capabilities for a client.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ElicitationCapabilities {
    /// Support for form-based elicitation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub form: Option<HashMap<String, Value>>,
    /// Support for URL-based elicitation (new in 2025-11-25).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<HashMap<String, Value>>,
}

/// Sampling capabilities for a client.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SamplingCapabilities {
    /// Support for context inclusion (soft-deprecated).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<HashMap<String, Value>>,
    /// Support for tool use in sampling (new in 2025-11-25).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<HashMap<String, Value>>,
}

/// Roots capabilities for a client.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RootsCapabilities {
    /// Support for roots/list_changed notifications.
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Task capabilities for a client.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ClientTaskCapabilities {
    /// Support for tasks/list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list: Option<HashMap<String, Value>>,
    /// Support for tasks/cancel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancel: Option<HashMap<String, Value>>,
    /// Requests that can be augmented with tasks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requests: Option<ClientTaskRequests>,
}

/// Client-side task-augmented request capabilities.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ClientTaskRequests {
    /// Support for task-augmented sampling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<ClientTaskSamplingRequests>,
    /// Support for task-augmented elicitation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<ClientTaskElicitationRequests>,
}

/// Client task-augmented sampling request capabilities.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ClientTaskSamplingRequests {
    /// Support for task-augmented sampling/createMessage.
    #[serde(rename = "createMessage", skip_serializing_if = "Option::is_none")]
    pub create_message: Option<HashMap<String, Value>>,
}

/// Client task-augmented elicitation request capabilities.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ClientTaskElicitationRequests {
    /// Support for task-augmented elicitation/create.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create: Option<HashMap<String, Value>>,
}

/// Capabilities supported by a server.
///
/// Per MCP 2025-11-25, server capabilities are:
/// `tools`, `resources`, `prompts`, `logging`, `completions`, `tasks`, `experimental`
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ServerCapabilities {
    /// Support for tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolCapabilities>,
    /// Support for resources.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceCapabilities>,
    /// Support for prompts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<PromptCapabilities>,
    /// Support for logging.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging: Option<HashMap<String, Value>>,
    /// Support for argument autocompletion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completions: Option<HashMap<String, Value>>,
    /// Support for task-augmented requests (experimental in 2025-11-25).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tasks: Option<ServerTaskCapabilities>,
    /// Experimental capabilities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<HashMap<String, Value>>,
}

/// Tool capabilities for a server.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ToolCapabilities {
    /// Support for tools/list_changed notifications.
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Resource capabilities for a server.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ResourceCapabilities {
    /// Support for resources/subscribe and notifications/resources/updated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscribe: Option<bool>,
    /// Support for resources/list_changed notifications.
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Prompt capabilities for a server.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PromptCapabilities {
    /// Support for prompts/list_changed notifications.
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Server-side task capabilities.
///
/// Per MCP 2025-11-25: `{ list?: object, cancel?: object, requests?: { tools?: { call?: object } } }`
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ServerTaskCapabilities {
    /// Support for tasks/list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list: Option<HashMap<String, Value>>,
    /// Support for tasks/cancel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancel: Option<HashMap<String, Value>>,
    /// Request types that can be augmented with tasks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requests: Option<ServerTaskRequests>,
}

/// Server-side task-augmented request capabilities.
///
/// Per spec: `{ tools?: { call?: object } }`
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ServerTaskRequests {
    /// Task support for tool-related requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ServerTaskToolRequests>,
}

/// Server task-augmented tool request capabilities.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ServerTaskToolRequests {
    /// Whether the server supports task-augmented tools/call requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call: Option<HashMap<String, Value>>,
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_include_context_serde() {
        // Verify camelCase serialization
        let json = serde_json::to_string(&IncludeContext::ThisServer).unwrap();
        assert_eq!(json, "\"thisServer\"");

        let json = serde_json::to_string(&IncludeContext::AllServers).unwrap();
        assert_eq!(json, "\"allServers\"");

        let json = serde_json::to_string(&IncludeContext::None).unwrap();
        assert_eq!(json, "\"none\"");

        // Round-trip
        let parsed: IncludeContext = serde_json::from_str("\"thisServer\"").unwrap();
        assert_eq!(parsed, IncludeContext::ThisServer);
    }

    #[test]
    fn test_tool_choice_mode_optional() {
        // mode is optional, should serialize empty when None
        let tc = ToolChoice { mode: None };
        let json = serde_json::to_string(&tc).unwrap();
        assert_eq!(json, "{}");

        // Explicit mode
        let tc = ToolChoice {
            mode: Some(ToolChoiceMode::Required),
        };
        let json = serde_json::to_string(&tc).unwrap();
        assert!(json.contains("\"required\""));
    }

    #[test]
    fn test_model_hint_name_optional() {
        let hint = ModelHint { name: None };
        let json = serde_json::to_string(&hint).unwrap();
        assert_eq!(json, "{}");

        let hint = ModelHint {
            name: Some("claude".into()),
        };
        let json = serde_json::to_string(&hint).unwrap();
        assert!(json.contains("\"claude\""));
    }

    #[test]
    fn test_task_status_serde() {
        let json = serde_json::to_string(&TaskStatus::InputRequired).unwrap();
        assert_eq!(json, "\"input_required\"");

        let json = serde_json::to_string(&TaskStatus::Working).unwrap();
        assert_eq!(json, "\"working\"");
    }

    #[test]
    fn test_create_message_request_default() {
        // Verify Default works (used in builder pattern)
        let req = CreateMessageRequest {
            messages: vec![SamplingMessage::user("hello")],
            max_tokens: 100,
            ..Default::default()
        };
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.max_tokens, 100);
        assert!(req.tools.is_none());
    }

    #[test]
    fn test_sampling_message_content_single_or_array() {
        // Single content
        let msg = SamplingMessage::user("hello");
        let json = serde_json::to_string(&msg).unwrap();
        // Single should be an object, not array
        assert!(json.contains("\"text\":\"hello\""));

        // Round-trip
        let parsed: SamplingMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.content.as_text(), Some("hello"));

        // Array content
        let json_array = r#"{"role":"user","content":[{"type":"text","text":"hello"},{"type":"text","text":"world"}]}"#;
        let parsed: SamplingMessage = serde_json::from_str(json_array).unwrap();
        match &parsed.content {
            SamplingContentBlock::Multiple(v) => assert_eq!(v.len(), 2),
            _ => panic!("Expected multiple content blocks"),
        }
    }

    #[test]
    fn test_server_capabilities_structure() {
        let caps = ServerCapabilities {
            tasks: Some(ServerTaskCapabilities {
                list: Some(HashMap::new()),
                cancel: Some(HashMap::new()),
                requests: Some(ServerTaskRequests {
                    tools: Some(ServerTaskToolRequests {
                        call: Some(HashMap::new()),
                    }),
                }),
            }),
            ..Default::default()
        };
        let json = serde_json::to_string(&caps).unwrap();
        let v: Value = serde_json::from_str(&json).unwrap();
        // Verify nested structure matches spec
        assert!(v["tasks"]["requests"]["tools"]["call"].is_object());
    }

    // C-3: ElicitAction and ElicitResult serde
    #[test]
    fn test_elicit_action_serde() {
        let cases = [
            (ElicitAction::Accept, "\"accept\""),
            (ElicitAction::Decline, "\"decline\""),
            (ElicitAction::Cancel, "\"cancel\""),
        ];
        for (action, expected) in cases {
            let json = serde_json::to_string(&action).unwrap();
            assert_eq!(json, expected);
            let parsed: ElicitAction = serde_json::from_str(expected).unwrap();
            assert_eq!(parsed, action);
        }
    }

    #[test]
    fn test_elicit_result_round_trip() {
        let result = ElicitResult {
            action: ElicitAction::Accept,
            content: Some(serde_json::json!({"name": "test"})),
            meta: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: ElicitResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.action, ElicitAction::Accept);
        assert!(parsed.content.is_some());

        // Decline with no content
        let decline = ElicitResult {
            action: ElicitAction::Decline,
            content: None,
            meta: None,
        };
        let json = serde_json::to_string(&decline).unwrap();
        assert!(!json.contains("\"content\""));
        let parsed: ElicitResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.action, ElicitAction::Decline);
        assert!(parsed.content.is_none());
    }

    // H-7: ServerCapabilities must NOT contain elicitation or sampling
    #[test]
    fn test_server_capabilities_no_elicitation_or_sampling() {
        let caps = ServerCapabilities::default();
        let json = serde_json::to_string(&caps).unwrap();
        assert!(!json.contains("elicitation"));
        assert!(!json.contains("sampling"));

        // Even fully populated
        let caps = ServerCapabilities {
            tools: Some(ToolCapabilities {
                list_changed: Some(true),
            }),
            resources: Some(ResourceCapabilities {
                subscribe: Some(true),
                list_changed: Some(true),
            }),
            prompts: Some(PromptCapabilities {
                list_changed: Some(true),
            }),
            logging: Some(HashMap::new()),
            completions: Some(HashMap::new()),
            tasks: Some(ServerTaskCapabilities::default()),
            experimental: Some(HashMap::new()),
        };
        let json = serde_json::to_string(&caps).unwrap();
        assert!(!json.contains("elicitation"));
        assert!(!json.contains("sampling"));
    }

    // H-8: SamplingMessage array content round-trip preserves array
    #[test]
    fn test_sampling_message_array_content_round_trip() {
        let json_array =
            r#"{"role":"user","content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}"#;
        let parsed: SamplingMessage = serde_json::from_str(json_array).unwrap();
        let re_serialized = serde_json::to_string(&parsed).unwrap();
        let re_parsed: Value = serde_json::from_str(&re_serialized).unwrap();
        assert!(re_parsed["content"].is_array());
        assert_eq!(re_parsed["content"].as_array().unwrap().len(), 2);
    }

    // H-10: All ToolChoiceMode variants
    #[test]
    fn test_tool_choice_mode_all_variants() {
        let cases = [
            (ToolChoiceMode::Auto, "\"auto\""),
            (ToolChoiceMode::None, "\"none\""),
            (ToolChoiceMode::Required, "\"required\""),
        ];
        for (mode, expected) in cases {
            let json = serde_json::to_string(&mode).unwrap();
            assert_eq!(json, expected);
            let parsed: ToolChoiceMode = serde_json::from_str(expected).unwrap();
            assert_eq!(parsed, mode);
        }
    }

    // CRITICAL-2: ElicitRequestParams custom serde - optional mode field
    #[test]
    fn test_elicit_request_params_form_without_mode() {
        // Per MCP 2025-11-25, mode is optional and defaults to "form"
        let json = r#"{"message":"Enter name","requestedSchema":{"type":"object"}}"#;
        let parsed: ElicitRequestParams = serde_json::from_str(json).unwrap();
        match &parsed {
            ElicitRequestParams::Form(params) => {
                assert_eq!(params.message, "Enter name");
            }
            ElicitRequestParams::Url(_) => panic!("expected Form variant"),
        }
    }

    #[test]
    fn test_elicit_request_params_form_with_explicit_mode() {
        let json = r#"{"mode":"form","message":"Enter name","requestedSchema":{"type":"object"}}"#;
        let parsed: ElicitRequestParams = serde_json::from_str(json).unwrap();
        match &parsed {
            ElicitRequestParams::Form(params) => {
                assert_eq!(params.message, "Enter name");
            }
            ElicitRequestParams::Url(_) => panic!("expected Form variant"),
        }
    }

    #[test]
    fn test_elicit_request_params_url_mode() {
        let json = r#"{"mode":"url","message":"Authenticate","url":"https://example.com/auth","elicitationId":"e-123"}"#;
        let parsed: ElicitRequestParams = serde_json::from_str(json).unwrap();
        match &parsed {
            ElicitRequestParams::Url(params) => {
                assert_eq!(params.message, "Authenticate");
                assert_eq!(params.url, "https://example.com/auth");
                assert_eq!(params.elicitation_id, "e-123");
            }
            ElicitRequestParams::Form(_) => panic!("expected Url variant"),
        }
    }

    #[test]
    fn test_elicit_request_params_form_round_trip() {
        let params = ElicitRequestParams::Form(ElicitRequestFormParams {
            message: "Enter details".into(),
            requested_schema: serde_json::json!({"type": "object", "properties": {"name": {"type": "string"}}}),
            task: None,
            meta: None,
        });
        let json = serde_json::to_string(&params).unwrap();
        // Serialized output must include mode: "form"
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["mode"], "form");
        // Round-trip
        let parsed: ElicitRequestParams = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, params);
    }

    #[test]
    fn test_elicit_request_params_url_round_trip() {
        let params = ElicitRequestParams::Url(ElicitRequestURLParams {
            message: "Please authenticate".into(),
            url: "https://example.com/oauth".into(),
            elicitation_id: "elicit-456".into(),
            task: None,
            meta: None,
        });
        let json = serde_json::to_string(&params).unwrap();
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["mode"], "url");
        let parsed: ElicitRequestParams = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, params);
    }

    // M-6: All TaskStatus variants
    #[test]
    fn test_task_status_all_variants() {
        let cases = [
            (TaskStatus::Cancelled, "\"cancelled\""),
            (TaskStatus::Completed, "\"completed\""),
            (TaskStatus::Failed, "\"failed\""),
            (TaskStatus::InputRequired, "\"input_required\""),
            (TaskStatus::Working, "\"working\""),
        ];
        for (status, expected) in cases {
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, expected);
            let parsed: TaskStatus = serde_json::from_str(expected).unwrap();
            assert_eq!(parsed, status);
        }
    }
}
