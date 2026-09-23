//! Request/response/notification routing types
//!
//! This module contains the top-level enums that route different types of
//! MCP requests and notifications between clients and servers.

// The enums below are deprecated; their own derives and impls still use them.
#![allow(deprecated)]

use serde::{Deserialize, Serialize};

use super::{
    completion::CompleteRequestParams,
    elicitation::{ElicitRequestParams, ElicitationCompleteNotification},
    initialization::{InitializeRequest, InitializedNotification},
    logging::{LoggingNotification, ProgressNotification, SetLevelRequest},
    ping::PingParams,
    prompts::{GetPromptRequest, ListPromptsRequest},
    resources::{
        ListResourceTemplatesRequest, ListResourcesRequest, ReadResourceRequest,
        ResourceUpdatedNotification, SubscribeRequest, UnsubscribeRequest,
    },
    roots::{ListRootsRequest, RootsListChangedNotification},
    sampling::CreateMessageRequest,
    tasks::{
        CancelTaskRequest, GetTaskPayloadRequest, GetTaskRequest, ListTasksRequest,
        TaskStatusNotification,
    },
    tools::{CallToolRequest, ListToolsRequest},
};

/// Client-initiated request
///
/// # Deprecated: not the wire shape
///
/// Internally tagged, so a variant's params sit beside `method` rather than
/// under `params`: `CallTool` serializes as `{"method":"tools/call","name":..}`
/// and a real `tools/call` request fails to parse. Route on the JSON-RPC
/// `method` and deserialize `params` into the variant's type instead. Kept
/// only so existing code compiles; it will be removed in 4.0.
#[deprecated(
    since = "3.5.0",
    note = "not the JSON-RPC wire shape; route on `method` and deserialize `params` into the matching type"
)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method")]
#[allow(clippy::large_enum_variant)] // InitializeRequest is the MCP handshake payload; boxing would change wire API.
pub enum ClientRequest {
    /// Initialize the connection
    #[serde(rename = "initialize")]
    Initialize(InitializeRequest),

    /// List available tools
    #[serde(rename = "tools/list")]
    ListTools(ListToolsRequest),

    /// Call a tool
    #[serde(rename = "tools/call")]
    CallTool(CallToolRequest),

    /// List available prompts
    #[serde(rename = "prompts/list")]
    ListPrompts(ListPromptsRequest),

    /// Get a specific prompt
    #[serde(rename = "prompts/get")]
    GetPrompt(GetPromptRequest),

    /// List available resources
    #[serde(rename = "resources/list")]
    ListResources(ListResourcesRequest),

    /// List resource templates
    #[serde(rename = "resources/templates/list")]
    ListResourceTemplates(ListResourceTemplatesRequest),

    /// Read a resource
    #[serde(rename = "resources/read")]
    ReadResource(ReadResourceRequest),

    /// Subscribe to resource updates
    #[serde(rename = "resources/subscribe")]
    Subscribe(SubscribeRequest),

    /// Unsubscribe from resource updates
    #[serde(rename = "resources/unsubscribe")]
    Unsubscribe(UnsubscribeRequest),

    /// Set logging level
    #[serde(rename = "logging/setLevel")]
    SetLevel(SetLevelRequest),

    /// Complete argument
    #[serde(rename = "completion/complete")]
    Complete(CompleteRequestParams),

    /// Ping to check connection
    #[serde(rename = "ping")]
    Ping(PingParams),

    /// Get task status (Tasks API, MCP 2025-11-25, schema.ts:2520-2527)
    #[serde(rename = "tasks/get")]
    TasksGet(GetTaskRequest),

    /// Get task result payload (Tasks API)
    #[serde(rename = "tasks/result")]
    TasksResult(GetTaskPayloadRequest),

    /// List tasks (Tasks API)
    #[serde(rename = "tasks/list")]
    TasksList(ListTasksRequest),

    /// Cancel a task (Tasks API). Distinct from `notifications/cancelled`,
    /// which targets non-task requests per spec.
    #[serde(rename = "tasks/cancel")]
    TasksCancel(CancelTaskRequest),
}

/// Server-initiated request
///
/// # Deprecated: not the wire shape
///
/// Internally tagged, so a variant's params sit beside `method` rather than
/// under `params`: `CallTool` serializes as `{"method":"tools/call","name":..}`
/// and a real `tools/call` request fails to parse. Route on the JSON-RPC
/// `method` and deserialize `params` into the variant's type instead. Kept
/// only so existing code compiles; it will be removed in 4.0.
#[deprecated(
    since = "3.5.0",
    note = "not the JSON-RPC wire shape; route on `method` and deserialize `params` into the matching type"
)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method")]
pub enum ServerRequest {
    /// Ping to check connection
    #[serde(rename = "ping")]
    Ping(PingParams),

    /// Create a message (sampling) - server requests LLM sampling from client
    #[serde(rename = "sampling/createMessage")]
    CreateMessage(CreateMessageRequest),

    /// List filesystem roots - server requests root URIs from client
    #[serde(rename = "roots/list")]
    ListRoots(ListRootsRequest),

    /// Elicit user input
    #[serde(rename = "elicitation/create")]
    ElicitationCreate(ElicitRequestParams),
}

/// Client-sent notification
///
/// Per MCP 2025-11-25 (`schema.ts:2535`), `CancelledNotification` and
/// `TaskStatusNotification` are bidirectional — they appear in both
/// `ClientNotification` and `ServerNotification`.
///
/// # Deprecated: not the wire shape
///
/// Internally tagged, so a variant's params sit beside `method` rather than
/// under `params`: `CallTool` serializes as `{"method":"tools/call","name":..}`
/// and a real `tools/call` request fails to parse. Route on the JSON-RPC
/// `method` and deserialize `params` into the variant's type instead. Kept
/// only so existing code compiles; it will be removed in 4.0.
#[deprecated(
    since = "3.5.0",
    note = "not the JSON-RPC wire shape; route on `method` and deserialize `params` into the matching type"
)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method")]
pub enum ClientNotification {
    /// Connection initialized
    #[serde(rename = "notifications/initialized")]
    Initialized(InitializedNotification),

    /// Roots list changed
    #[serde(rename = "notifications/roots/list_changed")]
    RootsListChanged(RootsListChangedNotification),

    /// Request cancellation (bidirectional per spec)
    #[serde(rename = "notifications/cancelled")]
    Cancelled(CancelledNotification),

    /// Task status change (bidirectional per spec)
    #[serde(rename = "notifications/tasks/status")]
    TaskStatus(TaskStatusNotification),
}

/// Server-sent notification
///
/// # Deprecated: not the wire shape
///
/// Internally tagged, so a variant's params sit beside `method` rather than
/// under `params`: `CallTool` serializes as `{"method":"tools/call","name":..}`
/// and a real `tools/call` request fails to parse. Route on the JSON-RPC
/// `method` and deserialize `params` into the variant's type instead. Kept
/// only so existing code compiles; it will be removed in 4.0.
#[deprecated(
    since = "3.5.0",
    note = "not the JSON-RPC wire shape; route on `method` and deserialize `params` into the matching type"
)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method")]
pub enum ServerNotification {
    /// Log message
    #[serde(rename = "notifications/message")]
    Message(LoggingNotification),

    /// Progress update for a request
    #[serde(rename = "notifications/progress")]
    Progress(ProgressNotification),

    /// Resource updated
    #[serde(rename = "notifications/resources/updated")]
    ResourceUpdated(ResourceUpdatedNotification),

    /// Resource list changed
    #[serde(rename = "notifications/resources/list_changed")]
    ResourceListChanged,

    /// Request cancellation
    #[serde(rename = "notifications/cancelled")]
    Cancelled(CancelledNotification),

    /// Prompts list changed
    #[serde(rename = "notifications/prompts/list_changed")]
    PromptsListChanged,

    /// Tools list changed
    #[serde(rename = "notifications/tools/list_changed")]
    ToolsListChanged,

    /// Roots list changed
    #[serde(rename = "notifications/roots/list_changed")]
    RootsListChanged,

    /// Elicitation completed (MCP 2025-11-25, schema.ts:2562-2570)
    #[serde(rename = "notifications/elicitation/complete")]
    ElicitationComplete(ElicitationCompleteNotification),

    /// Task status change (bidirectional per spec)
    #[serde(rename = "notifications/tasks/status")]
    TaskStatus(TaskStatusNotification),
}

/// Cancellation notification.
///
/// Per MCP 2025-11-25, `requestId` is optional. It MUST be provided when
/// cancelling non-task requests; tasks use `tasks/cancel` instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelledNotification {
    /// Request ID that was cancelled
    #[serde(rename = "requestId", default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<super::core::RequestId>,
    /// Optional reason for cancellation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Optional metadata per the current MCP specification
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}
