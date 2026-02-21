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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum ElicitRequestParams {
    /// Form elicitation (structured input)
    Form(ElicitRequestFormParams),
    /// URL elicitation (out-of-band interaction)
    Url(ElicitRequestURLParams),
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ElicitAction {
    /// User accepted the request.
    Accept,
    /// User declined the request.
    Decline,
    /// User cancelled or dismissed the request.
    Cancel,
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

/// Context inclusion mode for sampling.
///
/// `thisServer` and `allServers` are soft-deprecated in 2025-11-25.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ToolChoiceMode {
    /// Model decides whether to use tools (default).
    Auto,
    /// Model MUST NOT use any tools.
    None,
    /// Model MUST use at least one tool.
    Required,
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
}
