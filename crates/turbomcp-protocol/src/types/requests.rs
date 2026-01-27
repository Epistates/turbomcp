//! Request/response/notification routing types
//!
//! This module contains the top-level enums that route different types of
//! MCP requests and notifications between clients and servers.

use serde::{Deserialize, Serialize};

use super::{
    completion::CompleteRequestParams,
    elicitation::ElicitRequestParams,
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
    tools::{CallToolRequest, ListToolsRequest},
};

/// Client-initiated request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method")]
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
}

/// Server-initiated request
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method")]
pub enum ClientNotification {
    /// Connection initialized
    #[serde(rename = "notifications/initialized")]
    Initialized(InitializedNotification),

    /// Roots list changed
    #[serde(rename = "notifications/roots/list_changed")]
    RootsListChanged(RootsListChangedNotification),
}

/// Server-sent notification
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
}

/// Cancellation notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelledNotification {
    /// Request ID that was cancelled
    #[serde(rename = "requestId")]
    pub request_id: super::core::RequestId,
    /// Optional reason for cancellation
    pub reason: Option<String>,
}
