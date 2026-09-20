//! MCP Protocol types for Tasks, Elicitation, and Sampling (MCP 2025-11-25).
//!
//! This module provides the specialized types introduced in the MCP 2025-11-25
//! specification for advanced protocol features.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(not(feature = "std"))]
use alloc::{collections::BTreeMap as HashMap, format, string::String, vec::Vec};
#[cfg(feature = "std")]
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

impl core::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
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

impl ElicitRequestParams {
    /// Create a form-mode elicitation request with no task/meta.
    #[must_use]
    pub fn form(message: impl Into<String>, requested_schema: Value) -> Self {
        Self::Form(ElicitRequestFormParams {
            message: message.into(),
            requested_schema,
            task: None,
            meta: None,
        })
    }

    /// Create a URL-mode elicitation request with no task/meta.
    #[must_use]
    pub fn url(
        message: impl Into<String>,
        url: impl Into<String>,
        elicitation_id: impl Into<String>,
    ) -> Self {
        Self::Url(ElicitRequestURLParams {
            message: message.into(),
            url: url.into(),
            elicitation_id: elicitation_id.into(),
            task: None,
            meta: None,
        })
    }

    /// Human-readable message common to both variants.
    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            Self::Form(p) => &p.message,
            Self::Url(p) => &p.message,
        }
    }

    /// Task metadata common to both variants.
    #[must_use]
    pub fn task(&self) -> Option<&TaskMetadata> {
        match self {
            Self::Form(p) => p.task.as_ref(),
            Self::Url(p) => p.task.as_ref(),
        }
    }

    /// Extension metadata (`_meta`) common to both variants.
    #[must_use]
    pub fn meta(&self) -> Option<&HashMap<String, Value>> {
        match self {
            Self::Form(p) => p.meta.as_ref(),
            Self::Url(p) => p.meta.as_ref(),
        }
    }
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

        match value.get("mode") {
            None => {
                let params: ElicitRequestFormParams =
                    serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                Ok(Self::Form(params))
            }
            Some(Value::String(mode)) if mode == "form" => {
                let params: ElicitRequestFormParams =
                    serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                Ok(Self::Form(params))
            }
            Some(Value::String(mode)) if mode == "url" => {
                let params: ElicitRequestURLParams =
                    serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                Ok(Self::Url(params))
            }
            Some(Value::String(mode)) => Err(serde::de::Error::custom(format!(
                "unsupported elicitation mode `{mode}`"
            ))),
            Some(_) => Err(serde::de::Error::custom(
                "elicitation mode must be a string when present",
            )),
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

impl core::fmt::Display for ElicitAction {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
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

impl CreateMessageRequest {
    /// Check the message sequence against MCP §Message Content Constraints.
    ///
    /// Two rules, both MUSTs, both carrying `-32602` when broken:
    ///
    /// - A message containing any `tool_result` block must contain **only**
    ///   `tool_result` blocks. Provider APIs give tool results a dedicated role
    ///   (OpenAI's `tool`, Gemini's `function`), so a mixed message has no
    ///   representation to translate into.
    /// - Every assistant message carrying `tool_use` blocks must be followed by
    ///   a user message consisting entirely of `tool_result` blocks, one per
    ///   tool use, before any other message.
    ///
    /// Catching this here turns what would otherwise surface as an opaque 400
    /// from Anthropic, OpenAI or Gemini — three messages downstream of the
    /// mistake — into the invalid-params error the spec prescribes.
    ///
    /// Version-agnostic: `tool_use` and `tool_result` do not exist on the
    /// 2025-06-18 wire, so on that wire this cannot fire.
    pub fn validate(&self) -> Result<(), String> {
        // Tool uses from the immediately preceding assistant message, still
        // waiting to be answered.
        let mut pending: Vec<&str> = Vec::new();

        for message in &self.messages {
            let blocks = message.content.to_vec();
            let has_tool_result = blocks
                .iter()
                .any(|block| matches!(block, SamplingContent::ToolResult(_)));

            if has_tool_result {
                if message.role != Role::User {
                    return Err(format!(
                        "tool_result content is only valid in a user message, found in {} message",
                        message.role
                    ));
                }
                if !blocks
                    .iter()
                    .all(|block| matches!(block, SamplingContent::ToolResult(_)))
                {
                    return Err(
                        "Tool results mixed with other content: a message containing tool results \
                         must contain only tool results"
                            .to_string(),
                    );
                }

                for block in &blocks {
                    let SamplingContent::ToolResult(result) = block else {
                        continue;
                    };
                    match pending
                        .iter()
                        .position(|id| *id == result.tool_use_id.as_str())
                    {
                        Some(index) => {
                            pending.swap_remove(index);
                        }
                        None => {
                            return Err(format!(
                                "tool_result with toolUseId '{}' has no matching tool_use in the \
                                 preceding assistant message",
                                result.tool_use_id
                            ));
                        }
                    }
                }
            }

            // Anything other than the answering user message closes the window.
            if !pending.is_empty() {
                return Err("Tool result missing in request".to_string());
            }

            for block in &blocks {
                let SamplingContent::ToolUse(tool_use) = block else {
                    continue;
                };
                if message.role != Role::Assistant {
                    return Err(format!(
                        "tool_use content is only valid in an assistant message, found in {} message",
                        message.role
                    ));
                }
                pending.push(tool_use.id.as_str());
            }
        }

        if pending.is_empty() {
            Ok(())
        } else {
            // A trailing tool_use turn is the normal shape of "now run these
            // tools" only when the *client* produces it; a request ending there
            // is asking the model to continue past an unresolved call.
            Err("Tool result missing in request".to_string())
        }
    }
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

impl core::fmt::Display for IncludeContext {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
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

impl core::fmt::Display for ToolChoiceMode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
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
/// `role`, `content` (as `SamplingContentBlock`), `model`, and `stopReason`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CreateMessageResult {
    /// The role of the generated message.
    pub role: Role,
    /// The sampled content (single block or array per `SamplingMessage`).
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
///
/// Per MCP 2025-11-25: `roots`, `sampling`, `elicitation`, `tasks`, `experimental`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ClientCapabilities {
    /// Support for listing roots.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roots: Option<RootsCapabilities>,
    /// Support for LLM sampling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<SamplingCapabilities>,
    /// Support for elicitation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<ElicitationCapabilities>,
    /// Support for the Tasks API (MCP 2025-11-25 draft, SEP-1686).
    ///
    /// When present, indicates the client can receive task-augmented requests
    /// (e.g. `sampling/createMessage`, `elicitation/create`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tasks: Option<ClientTasksCapabilities>,
    /// Draft extensions supported by the client.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<HashMap<String, Value>>,
    /// Experimental, non-standard capabilities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<HashMap<String, Value>>,
}

/// Elicitation capabilities per MCP 2025-11-25.
///
/// Supports two modes:
/// - `form`: in-band structured data collection (default if empty object).
/// - `url`: out-of-band interactions (OAuth, credentials, payments).
///
/// Per spec, an empty object (`{}`) is equivalent to declaring support for
/// `form` mode only.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ElicitationCapabilities {
    /// Form-mode elicitation support.
    ///
    /// Per spec, an empty capabilities object defaults to form support.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub form: Option<ElicitationFormCapabilities>,
    /// URL-mode elicitation support (MCP 2025-11-25).
    ///
    /// For sensitive interactions (OAuth, credentials, payments).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<ElicitationUrlCapabilities>,
    /// Whether the client performs JSON schema validation on elicitation responses.
    ///
    /// When `true`, the client validates user input against the provided schema
    /// before sending. (TurboMCP extension — not part of the MCP specification.)
    #[serde(rename = "schemaValidation", skip_serializing_if = "Option::is_none")]
    pub schema_validation: Option<bool>,
}

impl ElicitationCapabilities {
    /// Both form and URL support.
    #[must_use]
    pub fn full() -> Self {
        Self {
            form: Some(ElicitationFormCapabilities {}),
            url: Some(ElicitationUrlCapabilities {}),
            schema_validation: None,
        }
    }

    /// Form-mode support only.
    #[must_use]
    pub fn form_only() -> Self {
        Self {
            form: Some(ElicitationFormCapabilities {}),
            url: None,
            schema_validation: None,
        }
    }

    /// Whether form-mode elicitation is supported.
    ///
    /// Per spec, an empty capabilities object defaults to form support.
    #[must_use]
    pub fn supports_form(&self) -> bool {
        self.form.is_some() || (self.form.is_none() && self.url.is_none())
    }

    /// Whether URL-mode elicitation is supported.
    #[must_use]
    pub fn supports_url(&self) -> bool {
        self.url.is_some()
    }

    /// Enable schema validation (TurboMCP extension).
    #[must_use]
    pub fn with_schema_validation(mut self) -> Self {
        self.schema_validation = Some(true);
        self
    }

    /// Disable schema validation (TurboMCP extension).
    #[must_use]
    pub fn without_schema_validation(mut self) -> Self {
        self.schema_validation = Some(false);
        self
    }
}

/// Form-mode elicitation support.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ElicitationFormCapabilities {}

/// URL-mode elicitation support.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ElicitationUrlCapabilities {}

/// Sampling capabilities for a client.
///
/// Per MCP 2025-11-25: `{ context?: {}, tools?: {} }`.
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
    /// Support for `roots/list_changed` notifications.
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// A filesystem boundary the client has granted the server access to.
///
/// Canonical home for the type so `RequestContext::list_roots` can return it
/// without `turbomcp-core` depending on `turbomcp-protocol`; the latter
/// re-exports it from `types::roots` so the original path keeps working.
///
/// The `_meta` field keeps its original name and `Value` shape rather than
/// adopting this crate's `meta: Option<HashMap<..>>` convention, because
/// clients construct `Root` directly in their roots handlers and renaming it
/// would break them for no protocol benefit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Root {
    /// Root URI, typically a `file://` URI.
    pub uri: crate::primitives::Uri,
    /// Optional human-readable name for this root.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Optional metadata per the current MCP specification.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
}

/// Result of a `roots/list` request.
///
/// Roots are never paginated, so there is no cursor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListRootsResult {
    /// Filesystem roots the client is exposing.
    pub roots: Vec<Root>,
    /// Optional metadata per the current MCP specification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
}

/// Client-side Tasks capabilities (MCP 2025-11-25 draft, SEP-1686).
///
/// Indicates which task operations and request types the client supports.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ClientTasksCapabilities {
    /// Support for `tasks/list`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list: Option<TasksListCapabilities>,
    /// Support for `tasks/cancel`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancel: Option<TasksCancelCapabilities>,
    /// Support for task-augmented requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requests: Option<ClientTasksRequestsCapabilities>,
}

/// Client-side task-augmented request capabilities.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ClientTasksRequestsCapabilities {
    /// Support for task-augmented `sampling/createMessage`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<TasksSamplingCapabilities>,
    /// Support for task-augmented `elicitation/create`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<TasksElicitationCapabilities>,
}

/// Task-augmented sampling request capabilities.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TasksSamplingCapabilities {
    /// Support for task-augmented `sampling/createMessage`.
    #[serde(rename = "createMessage", skip_serializing_if = "Option::is_none")]
    pub create_message: Option<TasksSamplingCreateMessageCapabilities>,
}

/// Task-augmented `sampling/createMessage` capability marker.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TasksSamplingCreateMessageCapabilities {}

/// Task-augmented elicitation request capabilities.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TasksElicitationCapabilities {
    /// Support for task-augmented `elicitation/create`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create: Option<TasksElicitationCreateCapabilities>,
}

/// Task-augmented `elicitation/create` capability marker.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TasksElicitationCreateCapabilities {}

/// Capabilities supported by a server.
///
/// Per MCP 2025-11-25: `tools`, `resources`, `prompts`, `logging`, `completions`,
/// `tasks`, `experimental`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ServerCapabilities {
    /// Support for tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapabilities>,
    /// Support for resources.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcesCapabilities>,
    /// Support for prompts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<PromptsCapabilities>,
    /// Support for logging.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging: Option<LoggingCapabilities>,
    /// Support for argument autocompletion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completions: Option<CompletionCapabilities>,
    /// Support for the Tasks API (MCP 2025-11-25 draft, SEP-1686).
    ///
    /// When present, indicates the server can receive task-augmented requests
    /// (e.g. `tools/call`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tasks: Option<ServerTasksCapabilities>,
    /// Draft extensions supported by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<HashMap<String, Value>>,
    /// Experimental, non-standard capabilities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<HashMap<String, Value>>,
}

/// Tools capabilities for a server.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ToolsCapabilities {
    /// Support for `tools/list_changed` notifications.
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Resources capabilities for a server.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ResourcesCapabilities {
    /// Support for `resources/subscribe` and `notifications/resources/updated`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscribe: Option<bool>,
    /// Support for `resources/list_changed` notifications.
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Prompts capabilities for a server.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PromptsCapabilities {
    /// Support for `prompts/list_changed` notifications.
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Logging capability marker.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LoggingCapabilities {}

/// Argument autocompletion capability marker.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CompletionCapabilities {}

/// Server-side Tasks capabilities (MCP 2025-11-25 draft, SEP-1686).
///
/// Indicates which task operations and request types the server supports.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ServerTasksCapabilities {
    /// Support for `tasks/list`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list: Option<TasksListCapabilities>,
    /// Support for `tasks/cancel`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancel: Option<TasksCancelCapabilities>,
    /// Support for task-augmented requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requests: Option<ServerTasksRequestsCapabilities>,
}

/// Server-side task-augmented request capabilities.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ServerTasksRequestsCapabilities {
    /// Support for task-augmented `tools/call`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<TasksToolsCapabilities>,
}

/// Task-augmented tools capabilities.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TasksToolsCapabilities {
    /// Support for task-augmented `tools/call`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call: Option<TasksToolsCallCapabilities>,
}

/// Task-augmented `tools/call` capability marker.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TasksToolsCallCapabilities {}

/// `tasks/list` capability marker.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TasksListCapabilities {}

/// `tasks/cancel` capability marker.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TasksCancelCapabilities {}

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
            tasks: Some(ServerTasksCapabilities {
                list: Some(TasksListCapabilities {}),
                cancel: Some(TasksCancelCapabilities {}),
                requests: Some(ServerTasksRequestsCapabilities {
                    tools: Some(TasksToolsCapabilities {
                        call: Some(TasksToolsCallCapabilities {}),
                    }),
                }),
            }),
            extensions: Some(HashMap::from([(
                "trace".to_string(),
                serde_json::json!({"version": "1"}),
            )])),
            ..Default::default()
        };
        let json = serde_json::to_string(&caps).unwrap();
        let v: Value = serde_json::from_str(&json).unwrap();
        // Verify nested structure matches spec
        assert!(v["tasks"]["requests"]["tools"]["call"].is_object());
        assert!(v["extensions"]["trace"].is_object());
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
            tools: Some(ToolsCapabilities {
                list_changed: Some(true),
            }),
            resources: Some(ResourcesCapabilities {
                subscribe: Some(true),
                list_changed: Some(true),
            }),
            prompts: Some(PromptsCapabilities {
                list_changed: Some(true),
            }),
            logging: Some(LoggingCapabilities {}),
            completions: Some(CompletionCapabilities {}),
            tasks: Some(ServerTasksCapabilities::default()),
            extensions: Some(HashMap::from([(
                "trace".to_string(),
                serde_json::json!({"version": "1"}),
            )])),
            experimental: Some(HashMap::new()),
        };
        let json = serde_json::to_string(&caps).unwrap();
        assert!(!json.contains("elicitation"));
        assert!(!json.contains("sampling"));
        assert!(json.contains("extensions"));
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
    fn test_elicit_request_params_rejects_unknown_mode() {
        let json =
            r#"{"mode":"unknown","message":"Enter name","requestedSchema":{"type":"object"}}"#;
        let err = serde_json::from_str::<ElicitRequestParams>(json).unwrap_err();
        assert!(err.to_string().contains("unsupported elicitation mode"));
    }

    #[test]
    fn test_elicit_request_params_rejects_non_string_mode() {
        let json = r#"{"mode":true,"message":"Enter name","requestedSchema":{"type":"object"}}"#;
        let err = serde_json::from_str::<ElicitRequestParams>(json).unwrap_err();
        assert!(err.to_string().contains("mode must be a string"));
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
