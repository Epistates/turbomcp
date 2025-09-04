//! # MCP Protocol Types
//!
//! This module contains all the type definitions for the Model Context Protocol
//! according to the 2025-06-18 specification.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use turbomcp_core::MessageId;

/// Protocol version string
pub type ProtocolVersion = String;

/// JSON-RPC request identifier
pub type RequestId = MessageId;

/// Progress token for tracking long-running operations
pub type ProgressToken = String;

/// URI string
pub type Uri = String;

/// MIME type
pub type MimeType = String;

/// Base64 encoded data
pub type Base64String = String;

// ============================================================================
// JSON-RPC Error Codes
// ============================================================================

/// Standard JSON-RPC error codes per specification
pub mod error_codes {
    /// Parse error - Invalid JSON was received by the server
    pub const PARSE_ERROR: i32 = -32700;
    /// Invalid Request - The JSON sent is not a valid Request object
    pub const INVALID_REQUEST: i32 = -32600;
    /// Method not found - The method does not exist / is not available
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// Invalid params - Invalid method parameter(s)
    pub const INVALID_PARAMS: i32 = -32602;
    /// Internal error - Internal JSON-RPC error
    pub const INTERNAL_ERROR: i32 = -32603;
}

/// JSON-RPC error structure per MCP 2025-06-18 specification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonRpcError {
    /// The error type that occurred
    pub code: i32,
    /// A short description of the error (should be limited to a concise single sentence)
    pub message: String,
    /// Additional information about the error (detailed error information, nested errors, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcError {
    /// Create a new JSON-RPC error
    pub fn new(code: i32, message: String) -> Self {
        Self {
            code,
            message,
            data: None,
        }
    }

    /// Create a new JSON-RPC error with additional data
    pub fn with_data(code: i32, message: String, data: serde_json::Value) -> Self {
        Self {
            code,
            message,
            data: Some(data),
        }
    }

    /// Create a parse error
    pub fn parse_error() -> Self {
        Self::new(error_codes::PARSE_ERROR, "Parse error".to_string())
    }

    /// Create an invalid request error
    pub fn invalid_request() -> Self {
        Self::new(error_codes::INVALID_REQUEST, "Invalid Request".to_string())
    }

    /// Create a method not found error
    pub fn method_not_found(method: &str) -> Self {
        Self::new(
            error_codes::METHOD_NOT_FOUND,
            format!("Method not found: {method}"),
        )
    }

    /// Create an invalid params error
    pub fn invalid_params(details: &str) -> Self {
        Self::new(
            error_codes::INVALID_PARAMS,
            format!("Invalid params: {details}"),
        )
    }

    /// Create an internal error
    pub fn internal_error(details: &str) -> Self {
        Self::new(
            error_codes::INTERNAL_ERROR,
            format!("Internal error: {details}"),
        )
    }
}

/// Cursor for pagination
pub type Cursor = String;

// ============================================================================
// Base Metadata Interface
// ============================================================================

/// Base interface for metadata with name (identifier) and title (display name) properties.
/// Per MCP specification 2025-06-18, this is the foundation for Tool, Resource, and Prompt metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseMetadata {
    /// Intended for programmatic or logical use, but used as a display name in past specs or fallback (if title isn't present).
    pub name: String,

    /// Intended for UI and end-user contexts — optimized to be human-readable and easily understood,
    /// even by those unfamiliar with domain-specific terminology.
    ///
    /// If not provided, the name should be used for display (except for Tool,
    /// where `annotations.title` should be given precedence over using `name`, if present).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Implementation information for MCP clients and servers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Implementation {
    /// Implementation name
    pub name: String,
    /// Implementation display title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Implementation version
    pub version: String,
}

/// General annotations that can be attached to various MCP objects
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Annotations {
    /// Audience-specific hints or information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<Vec<String>>,
    /// Priority level for ordering or importance
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<f64>,
    /// The moment the resource was last modified, as an ISO 8601 formatted string
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "lastModified")]
    pub last_modified: Option<String>,
    /// Additional custom annotations
    #[serde(flatten)]
    pub custom: HashMap<String, serde_json::Value>,
}

// ============================================================================
// Core Protocol Types
// ============================================================================

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

/// Client-initiated notification
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method")]
pub enum ClientNotification {
    /// Connection initialized
    #[serde(rename = "notifications/initialized")]
    Initialized(InitializedNotification),

    /// Progress update
    #[serde(rename = "notifications/progress")]
    Progress(ProgressNotification),

    /// Roots list changed
    #[serde(rename = "notifications/roots/list_changed")]
    RootsListChanged(RootsListChangedNotification),
}

/// Server-initiated notification
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method")]
pub enum ServerNotification {
    /// Log message
    #[serde(rename = "notifications/message")]
    Message(LoggingNotification),

    /// Resource updated
    #[serde(rename = "notifications/resources/updated")]
    ResourceUpdated(ResourceUpdatedNotification),

    /// Resource list changed
    #[serde(rename = "notifications/resources/list_changed")]
    ResourceListChanged,

    /// Progress update
    #[serde(rename = "notifications/progress")]
    Progress(ProgressNotification),

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

// ============================================================================
// Initialization
// ============================================================================

/// Initialize request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeRequest {
    /// Protocol version
    #[serde(rename = "protocolVersion")]
    pub protocol_version: ProtocolVersion,
    /// Client capabilities
    pub capabilities: ClientCapabilities,
    /// Client implementation info
    #[serde(rename = "clientInfo")]
    pub client_info: Implementation,
}

/// Initialize result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    /// Protocol version
    #[serde(rename = "protocolVersion")]
    pub protocol_version: ProtocolVersion,
    /// Server capabilities
    pub capabilities: ServerCapabilities,
    /// Server implementation info
    #[serde(rename = "serverInfo")]
    pub server_info: Implementation,
    /// Additional instructions for the client
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

/// Initialized notification (no parameters)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializedNotification;

// ============================================================================
// Capabilities
// ============================================================================

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

// ============================================================================
// Content Types
// ============================================================================

/// Content block union type per MCP 2025-06-18 specification
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    /// Text content
    #[serde(rename = "text")]
    Text(TextContent),
    /// Image content
    #[serde(rename = "image")]
    Image(ImageContent),
    /// Audio content
    #[serde(rename = "audio")]
    Audio(AudioContent),
    /// Resource link
    #[serde(rename = "resource_link")]
    ResourceLink(ResourceLink),
    /// Embedded resource
    #[serde(rename = "resource")]
    Resource(EmbeddedResource),
}

/// Compatibility alias for the old Content enum
pub type Content = ContentBlock;

/// Text content per MCP 2025-06-18 specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextContent {
    /// The text content of the message
    pub text: String,
    /// Optional annotations for the client
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
    /// General metadata field for extensions and custom data
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, serde_json::Value>>,
}

/// Image content per MCP 2025-06-18 specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageContent {
    /// The base64-encoded image data
    pub data: String,
    /// The MIME type of the image. Different providers may support different image types
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    /// Optional annotations for the client
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
    /// General metadata field for extensions and custom data
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, serde_json::Value>>,
}

/// Audio content per MCP 2025-06-18 specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioContent {
    /// The base64-encoded audio data
    pub data: String,
    /// The MIME type of the audio. Different providers may support different audio types
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    /// Optional annotations for the client
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
    /// General metadata field for extensions and custom data
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, serde_json::Value>>,
}

/// Resource link per MCP 2025-06-18 specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLink {
    /// Resource name (programmatic identifier)
    pub name: String,
    /// Display title for UI contexts (optional, falls back to name if not provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The URI of this resource
    pub uri: String,
    /// A description of what this resource represents
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The MIME type of this resource, if known
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Optional annotations for the client
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
    /// The size of the raw resource content, if known
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// General metadata field for extensions and custom data
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, serde_json::Value>>,
}

/// Embedded resource content per MCP 2025-06-18 specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddedResource {
    /// The embedded resource content (text or binary)
    pub resource: ResourceContent,
    /// Optional annotations for the client
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
    /// General metadata field for extensions and custom data
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, serde_json::Value>>,
}

/// Role in conversation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// User role
    User,
    /// Assistant role
    Assistant,
}

// ============================================================================
// Tool Types
// ============================================================================

/// Tool-specific annotations for additional tool information
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolAnnotations {
    /// Title for display purposes - takes precedence over name for UI
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Audience-specific information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<Vec<String>>,
    /// Priority for ordering
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<f64>,
    /// If true, the tool may perform destructive updates to its environment
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "destructiveHint")]
    pub destructive_hint: Option<bool>,
    /// If true, calling the tool repeatedly with same arguments has no additional effect
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "idempotentHint")]
    pub idempotent_hint: Option<bool>,
    /// If true, this tool may interact with an "open world" of external entities
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "openWorldHint")]
    pub open_world_hint: Option<bool>,
    /// If true, the tool does not modify its environment
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "readOnlyHint")]
    pub read_only_hint: Option<bool>,
    /// Additional custom annotations
    #[serde(flatten)]
    pub custom: HashMap<String, serde_json::Value>,
}

/// Tool definition per MCP 2025-06-18 specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    /// Tool name (programmatic identifier)
    pub name: String,

    /// Display title for UI contexts (optional, falls back to name if not provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Human-readable description of the tool
    /// This can be used by clients to improve the LLM's understanding of available tools
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// JSON Schema object defining the expected parameters for the tool
    #[serde(rename = "inputSchema")]
    pub input_schema: ToolInputSchema,

    /// Optional JSON Schema object defining the structure of the tool's output
    /// returned in the structuredContent field of a CallToolResult
    #[serde(rename = "outputSchema", skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<ToolOutputSchema>,

    /// Optional additional tool information
    /// Display name precedence order is: title, annotations.title, then name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,

    /// General metadata field for extensions and custom data
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, serde_json::Value>>,
}

/// Tool input schema definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInputSchema {
    /// Must be "object" for tool input schemas
    #[serde(rename = "type")]
    pub schema_type: String,
    /// Schema properties defining the tool parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, serde_json::Value>>,
    /// List of required property names
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
    /// Whether additional properties are allowed
    #[serde(
        rename = "additionalProperties",
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_properties: Option<bool>,
}

/// Tool output schema definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutputSchema {
    /// Must be "object" for tool output schemas
    #[serde(rename = "type")]
    pub schema_type: String,
    /// Schema properties defining the tool output structure
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, serde_json::Value>>,
    /// List of required property names
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
    /// Whether additional properties are allowed
    #[serde(
        rename = "additionalProperties",
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_properties: Option<bool>,
}

/// List tools request (no parameters)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListToolsRequest;

/// List tools result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListToolsResult {
    /// Available tools
    pub tools: Vec<Tool>,
    /// Optional continuation token
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Call tool request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolRequest {
    /// Tool name
    pub name: String,
    /// Tool arguments
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<HashMap<String, serde_json::Value>>,
}

/// Call tool result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolResult {
    /// Result content
    pub content: Vec<ContentBlock>,
    /// Whether the operation failed
    #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

// ============================================================================
// Prompt Types
// ============================================================================

/// Prompt definition per MCP 2025-06-18 specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt {
    /// Prompt name (programmatic identifier)
    pub name: String,

    /// Display title for UI contexts (optional, falls back to name if not provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// An optional description of what this prompt provides
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// A list of arguments to use for templating the prompt
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<PromptArgument>>,

    /// General metadata field for extensions and custom data
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, serde_json::Value>>,
}

/// Prompt argument definition per MCP 2025-06-18 specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptArgument {
    /// Argument name (programmatic identifier)
    pub name: String,

    /// Display title for UI contexts (optional, falls back to name if not provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// A human-readable description of the argument
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Whether this argument must be provided
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
}

/// Prompt input parameters
pub type PromptInput = HashMap<String, serde_json::Value>;

/// List prompts request (no parameters)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPromptsRequest;

/// List prompts result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPromptsResult {
    /// Available prompts
    pub prompts: Vec<Prompt>,
    /// Optional continuation token
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Get prompt request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPromptRequest {
    /// Prompt name
    pub name: String,
    /// Prompt arguments
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<PromptInput>,
}

/// Get prompt result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPromptResult {
    /// Prompt description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Prompt messages
    pub messages: Vec<PromptMessage>,
}

/// Prompt message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptMessage {
    /// Message role
    pub role: Role,
    /// Message content
    pub content: Content,
}

// ============================================================================
// Resource Types
// ============================================================================

/// Resource definition per MCP 2025-06-18 specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    /// Resource name (programmatic identifier)
    pub name: String,

    /// Display title for UI contexts (optional, falls back to name if not provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// The URI of this resource
    pub uri: String,

    /// A description of what this resource represents
    /// This can be used by clients to improve the LLM's understanding of available resources
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The MIME type of this resource, if known
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,

    /// Optional annotations for the client
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,

    /// The size of the raw resource content, in bytes (before base64 encoding or tokenization), if known
    /// This can be used by Hosts to display file sizes and estimate context window usage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,

    /// General metadata field for extensions and custom data
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, serde_json::Value>>,
}

/// Base resource contents interface
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceContents {
    /// The URI of this resource
    pub uri: String,
    /// The MIME type of this resource, if known
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// General metadata field for extensions and custom data
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, serde_json::Value>>,
}

/// Text resource contents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextResourceContents {
    /// The URI of this resource
    pub uri: String,
    /// The MIME type of this resource, if known
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// The text content (must only be set for text-representable data)
    pub text: String,
    /// General metadata field for extensions and custom data
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, serde_json::Value>>,
}

/// Binary resource contents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobResourceContents {
    /// The URI of this resource
    pub uri: String,
    /// The MIME type of this resource, if known
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Base64-encoded binary data
    pub blob: String,
    /// General metadata field for extensions and custom data
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, serde_json::Value>>,
}

/// Union type for resource contents (text or binary)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResourceContent {
    /// Text resource content
    Text(TextResourceContents),
    /// Binary resource content
    Blob(BlobResourceContents),
}

/// List resources request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResourcesRequest {
    /// Optional cursor for pagination
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// List resources result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResourcesResult {
    /// Available resources
    pub resources: Vec<Resource>,
    /// Optional continuation token
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Read resource request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadResourceRequest {
    /// Resource URI
    pub uri: Uri,
}

/// Read resource result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadResourceResult {
    /// Resource contents (can be text or binary)
    pub contents: Vec<ResourceContent>,
}

/// Subscribe to resource request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribeRequest {
    /// Resource URI
    pub uri: Uri,
}

/// Unsubscribe from resource request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsubscribeRequest {
    /// Resource URI
    pub uri: Uri,
}

/// Resource updated notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUpdatedNotification {
    /// Resource URI
    pub uri: Uri,
}

// ============================================================================
// Logging Types
// ============================================================================

/// Log level
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Debug level
    Debug,
    /// Info level
    Info,
    /// Notice level
    Notice,
    /// Warning level
    Warning,
    /// Error level
    Error,
    /// Critical level
    Critical,
    /// Alert level
    Alert,
    /// Emergency level
    Emergency,
}

/// Set log level request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetLevelRequest {
    /// Log level to set
    pub level: LogLevel,
}

/// Set log level result (no data)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetLevelResult;

/// Logging notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingNotification {
    /// Log level
    pub level: LogLevel,
    /// Log data
    pub data: serde_json::Value,
    /// Optional logger name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logger: Option<String>,
}

// ============================================================================
// Progress Types
// ============================================================================

/// Progress notification per MCP 2025-06-18 specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressNotification {
    /// The progress token which was given in the initial request
    /// Used to associate this notification with the request that is proceeding
    #[serde(rename = "progressToken")]
    pub progress_token: ProgressToken,
    /// The progress thus far. This should increase every time progress is made,
    /// even if the total is unknown
    pub progress: f64,
    /// Total number of items to process (or total progress required), if known
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<f64>,
    /// An optional message describing the current progress
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Cancellation notification per MCP 2025-06-18 specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelledNotification {
    /// The ID of the request to cancel
    /// This MUST correspond to the ID of a request previously issued in the same direction
    #[serde(rename = "requestId")]
    pub request_id: RequestId,
    /// An optional string describing the reason for the cancellation
    /// This MAY be logged or presented to the user
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

// ============================================================================
// Sampling Types
// ============================================================================

/// Create message request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMessageRequest {
    /// Messages to include
    pub messages: Vec<SamplingMessage>,
    /// Model preferences
    #[serde(rename = "modelPreferences", skip_serializing_if = "Option::is_none")]
    pub model_preferences: Option<ModelPreferences>,
    /// System prompt
    #[serde(rename = "systemPrompt", skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// Include context
    #[serde(rename = "includeContext", skip_serializing_if = "Option::is_none")]
    pub include_context: Option<IncludeContext>,
    /// Temperature
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Max tokens (required by MCP 2025-06-18 spec)
    #[serde(rename = "maxTokens")]
    pub max_tokens: u32,
    /// Stop sequences
    #[serde(rename = "stopSequences", skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// Metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

/// Model preferences
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPreferences {
    /// Preferred hints
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hints: Option<Vec<ModelHint>>,
    /// Cost priority
    #[serde(rename = "costPriority", skip_serializing_if = "Option::is_none")]
    pub cost_priority: Option<f64>,
    /// Speed priority
    #[serde(rename = "speedPriority", skip_serializing_if = "Option::is_none")]
    pub speed_priority: Option<f64>,
    /// Intelligence priority
    #[serde(
        rename = "intelligencePriority",
        skip_serializing_if = "Option::is_none"
    )]
    pub intelligence_priority: Option<f64>,
}

/// Model hint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelHint {
    /// Hint name (required by MCP 2025-06-18 spec)
    pub name: String,
}

/// Include context options
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum IncludeContext {
    /// No context
    None,
    /// This server only
    ThisServer,
    /// All servers
    AllServers,
}

/// Sampling message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplingMessage {
    /// Message role
    pub role: Role,
    /// Message content
    pub content: Content,
}

/// Create message result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMessageResult {
    /// Role of the created message
    pub role: Role,
    /// Content of the created message
    pub content: Content,
    /// Model used
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Stop reason
    #[serde(rename = "stopReason", skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
}

// ============================================================================
// ELICITATION (SERVER-INITIATED USER INPUT)
// ============================================================================

/// Primitive schema definition for elicitation requests
/// Only allows primitive types without nesting, as per MCP 2025-06-18 spec
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum PrimitiveSchemaDefinition {
    /// String field schema definition
    #[serde(rename = "string")]
    String {
        /// Field title
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Field description
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// String format (email, uri, date, date-time, etc.)
        #[serde(skip_serializing_if = "Option::is_none")]
        format: Option<String>,
        /// Minimum string length
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "minLength")]
        min_length: Option<u32>,
        /// Maximum string length
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "maxLength")]
        max_length: Option<u32>,
        /// Allowed enum values
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "enum")]
        enum_values: Option<Vec<String>>,
        /// Display names for enum values
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "enumNames")]
        enum_names: Option<Vec<String>>,
    },
    /// Number field schema definition
    #[serde(rename = "number")]
    Number {
        /// Field title
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Field description
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// Minimum value
        #[serde(skip_serializing_if = "Option::is_none")]
        minimum: Option<f64>,
        /// Maximum value
        #[serde(skip_serializing_if = "Option::is_none")]
        maximum: Option<f64>,
    },
    /// Integer field schema definition
    #[serde(rename = "integer")]
    Integer {
        /// Field title
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Field description
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// Minimum value
        #[serde(skip_serializing_if = "Option::is_none")]
        minimum: Option<i64>,
        /// Maximum value
        #[serde(skip_serializing_if = "Option::is_none")]
        maximum: Option<i64>,
    },
    /// Boolean field schema definition
    #[serde(rename = "boolean")]
    Boolean {
        /// Field title
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Field description
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// Default value
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<bool>,
    },
}

/// Elicitation schema - restricted subset of JSON Schema for primitive types only
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ElicitationSchema {
    /// Schema type (must be "object")
    #[serde(rename = "type")]
    pub schema_type: String,
    /// Schema properties (field definitions)
    pub properties: std::collections::HashMap<String, PrimitiveSchemaDefinition>,
    /// Required field names
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
}

impl ElicitationSchema {
    /// Create a new elicitation schema
    pub fn new() -> Self {
        Self {
            schema_type: "object".to_string(),
            properties: std::collections::HashMap::new(),
            required: None,
        }
    }

    /// Add a string property
    pub fn add_string_property<K: Into<String>>(
        mut self,
        name: K,
        required: bool,
        description: Option<String>,
    ) -> Self {
        let property = PrimitiveSchemaDefinition::String {
            title: None,
            description,
            format: None,
            min_length: None,
            max_length: None,
            enum_values: None,
            enum_names: None,
        };

        let name = name.into();
        self.properties.insert(name.clone(), property);

        if required {
            self.required.get_or_insert_with(Vec::new).push(name);
        }

        self
    }

    /// Add a number property
    pub fn add_number_property<K: Into<String>>(
        mut self,
        name: K,
        required: bool,
        description: Option<String>,
        min: Option<f64>,
        max: Option<f64>,
    ) -> Self {
        let property = PrimitiveSchemaDefinition::Number {
            title: None,
            description,
            minimum: min,
            maximum: max,
        };

        let name = name.into();
        self.properties.insert(name.clone(), property);

        if required {
            self.required.get_or_insert_with(Vec::new).push(name);
        }

        self
    }

    /// Add a boolean property
    pub fn add_boolean_property<K: Into<String>>(
        mut self,
        name: K,
        required: bool,
        description: Option<String>,
        default: Option<bool>,
    ) -> Self {
        let property = PrimitiveSchemaDefinition::Boolean {
            title: None,
            description,
            default,
        };

        let name = name.into();
        self.properties.insert(name.clone(), property);

        if required {
            self.required.get_or_insert_with(Vec::new).push(name);
        }

        self
    }

    /// Add an enum property
    pub fn add_enum_property<K: Into<String>>(
        mut self,
        name: K,
        required: bool,
        description: Option<String>,
        values: Vec<String>,
        names: Option<Vec<String>>,
    ) -> Self {
        let property = PrimitiveSchemaDefinition::String {
            title: None,
            description,
            format: None,
            min_length: None,
            max_length: None,
            enum_values: Some(values),
            enum_names: names,
        };

        let name = name.into();
        self.properties.insert(name.clone(), property);

        if required {
            self.required.get_or_insert_with(Vec::new).push(name);
        }

        self
    }
}

impl Default for ElicitationSchema {
    fn default() -> Self {
        Self::new()
    }
}

/// Parameters for elicitation/create request
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ElicitRequestParams {
    /// The message to present to the user
    pub message: String,
    /// JSON Schema defining the expected response structure
    pub requested_schema: ElicitationSchema,
}

/// Request to elicit user input (server-initiated)
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ElicitRequest {
    /// The method name (always "elicitation/create")
    pub method: String,
    /// Request parameters
    pub params: ElicitRequestParams,
}

impl ElicitRequest {
    /// Create a new elicit request
    pub fn new<M: Into<String>>(message: M, schema: ElicitationSchema) -> Self {
        Self {
            method: "elicitation/create".to_string(),
            params: ElicitRequestParams {
                message: message.into(),
                requested_schema: schema,
            },
        }
    }
}

/// Action taken by user in response to elicitation
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ElicitationAction {
    /// User submitted the form/confirmed the action
    Accept,
    /// User explicitly declined the action
    Decline,
    /// User dismissed without making an explicit choice
    Cancel,
}

/// Client's response to an elicitation request
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ElicitResult {
    /// The user action in response to the elicitation
    pub action: ElicitationAction,
    /// The submitted form data, only present when action is "accept"
    /// Contains values matching the requested schema
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<std::collections::HashMap<String, serde_json::Value>>,
    /// Optional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

impl ElicitResult {
    /// Create an accept response with content
    pub fn accept(content: std::collections::HashMap<String, serde_json::Value>) -> Self {
        Self {
            action: ElicitationAction::Accept,
            content: Some(content),
            _meta: None,
        }
    }

    /// Create a decline response
    pub fn decline() -> Self {
        Self {
            action: ElicitationAction::Decline,
            content: None,
            _meta: None,
        }
    }

    /// Create a cancel response
    pub fn cancel() -> Self {
        Self {
            action: ElicitationAction::Cancel,
            content: None,
            _meta: None,
        }
    }

    /// Add metadata
    pub fn with_meta(mut self, meta: serde_json::Value) -> Self {
        self._meta = Some(meta);
        self
    }
}

// ============================================================================
// COMPLETION (ARGUMENT AUTOCOMPLETION)
// ============================================================================

/// Information about the argument being completed
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ArgumentInfo {
    /// The name of the argument
    pub name: String,
    /// The value of the argument to use for completion matching
    pub value: String,
}

/// Context for completion requests
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct CompletionContext {
    /// Previously-resolved variables in a URI template or prompt
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<std::collections::HashMap<String, String>>,
}

/// Reference to a prompt for completion
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PromptReference {
    /// Reference type (always "ref/prompt")
    #[serde(rename = "type")]
    pub ref_type: String,
    /// The name of the prompt
    pub name: String,
    /// Human-readable title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl PromptReference {
    /// Create a new prompt reference
    pub fn new<N: Into<String>>(name: N) -> Self {
        Self {
            ref_type: "ref/prompt".to_string(),
            name: name.into(),
            title: None,
        }
    }

    /// Add a title to the reference
    pub fn with_title<T: Into<String>>(mut self, title: T) -> Self {
        self.title = Some(title.into());
        self
    }
}

/// Reference to a resource template for completion
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ResourceTemplateReference {
    /// Reference type (always "ref/resource")
    #[serde(rename = "type")]
    pub ref_type: String,
    /// The URI or URI template of the resource
    pub uri: String,
}

impl ResourceTemplateReference {
    /// Create a new resource template reference
    pub fn new<U: Into<String>>(uri: U) -> Self {
        Self {
            ref_type: "ref/resource".to_string(),
            uri: uri.into(),
        }
    }
}

/// Reference types for completion requests
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum CompletionReference {
    /// Reference to a prompt
    #[serde(rename = "ref/prompt")]
    Prompt(PromptReferenceData),
    /// Reference to a resource template
    #[serde(rename = "ref/resource")]
    ResourceTemplate(ResourceTemplateReferenceData),
}

/// Data for prompt reference (excluding the type field)
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PromptReferenceData {
    /// The name of the prompt
    pub name: String,
    /// Human-readable title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Data for resource template reference (excluding the type field)
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ResourceTemplateReferenceData {
    /// The URI or URI template of the resource
    pub uri: String,
}

/// Parameters for completion/complete request
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct CompleteRequestParams {
    /// The argument's information
    pub argument: ArgumentInfo,
    /// Reference to the item being completed
    #[serde(rename = "ref")]
    pub reference: CompletionReference,
    /// Additional context for completions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<CompletionContext>,
}

/// Request for argument completion
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct CompleteRequest {
    /// The method name (always "completion/complete")
    pub method: String,
    /// Request parameters
    pub params: CompleteRequestParams,
}

impl CompleteRequest {
    /// Create a new completion request
    pub fn new(argument: ArgumentInfo, reference: CompletionReference) -> Self {
        Self {
            method: "completion/complete".to_string(),
            params: CompleteRequestParams {
                argument,
                reference,
                context: None,
            },
        }
    }

    /// Add context to the request
    pub fn with_context(mut self, context: CompletionContext) -> Self {
        self.params.context = Some(context);
        self
    }
}

/// Completion response information
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CompletionResponse {
    /// Completion values (max 100 items)
    pub values: Vec<String>,
    /// Total number of completion options available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u32>,
    /// Whether there are additional completion options beyond those provided
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_more: Option<bool>,
}

impl CompletionResponse {
    /// Create a new completion response
    pub fn new(values: Vec<String>) -> Self {
        Self {
            values,
            total: None,
            has_more: None,
        }
    }

    /// Set total count
    pub fn with_total(mut self, total: u32) -> Self {
        self.total = Some(total);
        self
    }

    /// Set has_more flag
    pub fn with_has_more(mut self, has_more: bool) -> Self {
        self.has_more = Some(has_more);
        self
    }

    /// Create response with pagination info
    pub fn paginated(values: Vec<String>, total: u32, has_more: bool) -> Self {
        Self {
            values,
            total: Some(total),
            has_more: Some(has_more),
        }
    }
}

/// Server's response to a completion request
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct CompleteResult {
    /// Completion information
    pub completion: CompletionResponse,
    /// Optional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

impl CompleteResult {
    /// Create a new completion result
    pub fn new(completion: CompletionResponse) -> Self {
        Self {
            completion,
            _meta: None,
        }
    }

    /// Add metadata
    pub fn with_meta(mut self, meta: serde_json::Value) -> Self {
        self._meta = Some(meta);
        self
    }
}

// ============================================================================
// RESOURCE TEMPLATES (PARAMETERIZED RESOURCE ACCESS)
// ============================================================================

/// A template description for resources available on the server
/// Supports RFC 6570 URI template expansion
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResourceTemplate {
    /// Programmatic identifier
    pub name: String,
    /// URI template (RFC 6570)
    pub uri_template: String,
    /// Human-readable title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Description of what this template is for
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// MIME type for all resources matching this template (if uniform)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Optional annotations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
    /// Optional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

impl ResourceTemplate {
    /// Create a new resource template
    pub fn new<N: Into<String>, U: Into<String>>(name: N, uri_template: U) -> Self {
        Self {
            name: name.into(),
            uri_template: uri_template.into(),
            title: None,
            description: None,
            mime_type: None,
            annotations: None,
            _meta: None,
        }
    }

    /// Set the title
    pub fn with_title<T: Into<String>>(mut self, title: T) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the description
    pub fn with_description<D: Into<String>>(mut self, description: D) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the MIME type
    pub fn with_mime_type<M: Into<String>>(mut self, mime_type: M) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }

    /// Set annotations
    pub fn with_annotations(mut self, annotations: Annotations) -> Self {
        self.annotations = Some(annotations);
        self
    }

    /// Add metadata
    pub fn with_meta(mut self, meta: serde_json::Value) -> Self {
        self._meta = Some(meta);
        self
    }

    /// Create a file system template
    pub fn file_system<N: Into<String>>(name: N, base_path: &str) -> Self {
        Self::new(name, format!("file://{}/{{path}}", base_path))
            .with_title("File System Access")
            .with_description("Access files within the specified directory")
    }

    /// Create an API endpoint template
    pub fn api_endpoint<N: Into<String>>(name: N, base_url: &str) -> Self {
        Self::new(name, format!("{}/{{endpoint}}", base_url))
            .with_mime_type("application/json")
            .with_title("API Endpoint Access")
            .with_description("Access API endpoints")
    }

    /// Create a database query template
    pub fn database_query<N: Into<String>>(name: N) -> Self {
        Self::new(name, "db://query/{table}?{query*}")
            .with_title("Database Query")
            .with_description("Execute database queries")
    }
}

/// Parameters for listing resource templates
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct ListResourceTemplatesParams {
    /// Pagination cursor
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// Request to list resource templates
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ListResourceTemplatesRequest {
    /// Request parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<ListResourceTemplatesParams>,
}

impl ListResourceTemplatesRequest {
    /// Create a new list templates request
    pub fn new() -> Self {
        Self { params: None }
    }

    /// Create request with cursor for pagination
    pub fn with_cursor<C: Into<String>>(cursor: C) -> Self {
        Self {
            params: Some(ListResourceTemplatesParams {
                cursor: Some(cursor.into()),
            }),
        }
    }
}

impl Default for ListResourceTemplatesRequest {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of listing resource templates
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ListResourceTemplatesResult {
    /// Array of resource templates
    pub resource_templates: Vec<ResourceTemplate>,
    /// Pagination cursor for next page
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// Optional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

impl ListResourceTemplatesResult {
    /// Create a new result
    pub fn new(templates: Vec<ResourceTemplate>) -> Self {
        Self {
            resource_templates: templates,
            next_cursor: None,
            _meta: None,
        }
    }

    /// Create result with pagination
    pub fn paginated(templates: Vec<ResourceTemplate>, next_cursor: String) -> Self {
        Self {
            resource_templates: templates,
            next_cursor: Some(next_cursor),
            _meta: None,
        }
    }

    /// Add metadata
    pub fn with_meta(mut self, meta: serde_json::Value) -> Self {
        self._meta = Some(meta);
        self
    }
}

// ============================================================================
// PING PROTOCOL (CONNECTION HEALTH MONITORING)
// ============================================================================

/// Parameters for ping requests
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct PingParams {
    /// Optional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

impl PingParams {
    /// Create a new ping parameters
    pub fn new() -> Self {
        Self { _meta: None }
    }

    /// Add metadata
    pub fn with_meta(mut self, meta: serde_json::Value) -> Self {
        self._meta = Some(meta);
        self
    }
}

/// A ping request to check connection health
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PingRequest {
    /// The method name (always "ping")
    pub method: String,
    /// Request parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<PingParams>,
}

impl PingRequest {
    /// Create a new ping request
    pub fn new() -> Self {
        Self {
            method: "ping".to_string(),
            params: None,
        }
    }

    /// Create ping request with metadata
    pub fn with_meta(meta: serde_json::Value) -> Self {
        Self {
            method: "ping".to_string(),
            params: Some(PingParams::new().with_meta(meta)),
        }
    }
}

impl Default for PingRequest {
    fn default() -> Self {
        Self::new()
    }
}

/// Response to a ping request (usually empty)
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct PingResult {
    /// Optional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

impl PingResult {
    /// Create a new ping result
    pub fn new() -> Self {
        Self { _meta: None }
    }

    /// Create result with metadata
    pub fn with_meta(meta: serde_json::Value) -> Self {
        Self { _meta: Some(meta) }
    }
}

impl Default for PingResult {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Roots Types
// ============================================================================

/// Filesystem root
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Root {
    /// Root URI
    pub uri: Uri,
    /// Root name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// List roots request (no parameters)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListRootsRequest;

/// List roots result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListRootsResult {
    /// Available roots
    pub roots: Vec<Root>,
}

/// Roots list changed notification (no parameters)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootsListChangedNotification;

/// Empty result for operations that don't return data
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EmptyResult {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize() {
        let tool = Tool {
            name: "test_tool".to_string(),
            title: Some("Test Tool".to_string()),
            description: Some("A test tool".to_string()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: None,
                required: None,
                additional_properties: None,
            },
            output_schema: None,
            annotations: None,
            meta: None,
        };

        let json = serde_json::to_string(&tool).unwrap();
        let deserialized: Tool = serde_json::from_str(&json).unwrap();
        assert_eq!(tool.name, deserialized.name);
    }

    #[test]
    fn test_content_types() {
        let text_content = ContentBlock::Text(TextContent {
            text: "Hello, World!".to_string(),
            annotations: None,
            meta: None,
        });

        let json = serde_json::to_string(&text_content).unwrap();
        let _deserialized: ContentBlock = serde_json::from_str(&json).unwrap();

        // Test the compatibility alias
        let _compatible: Content = text_content;
    }

    #[test]
    fn test_elicitation_schema_builder() {
        let schema = ElicitationSchema::new()
            .add_string_property("username", true, Some("Your username".to_string()))
            .add_number_property(
                "age",
                false,
                Some("Your age".to_string()),
                Some(0.0),
                Some(150.0),
            )
            .add_boolean_property(
                "subscribe",
                true,
                Some("Subscribe to newsletter".to_string()),
                Some(false),
            )
            .add_enum_property(
                "role",
                true,
                Some("Your role".to_string()),
                vec!["admin".to_string(), "user".to_string(), "guest".to_string()],
                None,
            );

        assert_eq!(schema.schema_type, "object");
        assert_eq!(schema.properties.len(), 4);
        assert_eq!(schema.required.as_ref().unwrap().len(), 3);

        // Verify username property
        let username_prop = &schema.properties["username"];
        match username_prop {
            PrimitiveSchemaDefinition::String { description, .. } => {
                assert_eq!(description.as_ref().unwrap(), "Your username");
            }
            _ => panic!("Expected string property"),
        }

        // Verify age property
        let age_prop = &schema.properties["age"];
        match age_prop {
            PrimitiveSchemaDefinition::Number {
                minimum, maximum, ..
            } => {
                assert_eq!(*minimum, Some(0.0));
                assert_eq!(*maximum, Some(150.0));
            }
            _ => panic!("Expected number property"),
        }
    }

    #[test]
    fn test_elicit_request_serialization() {
        let schema = ElicitationSchema::new()
            .add_string_property("name", true, Some("Your name".to_string()))
            .add_boolean_property("confirm", true, None, Some(false));

        let request = ElicitRequest::new("Please provide your details", schema);

        // Serialize to JSON
        let json = serde_json::to_string(&request).unwrap();

        // Verify it contains expected structure
        assert!(json.contains("elicitation/create"));
        assert!(json.contains("Please provide your details"));
        assert!(json.contains("requestedSchema"));

        // Deserialize back
        let deserialized: ElicitRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.method, "elicitation/create");
        assert_eq!(deserialized.params.message, "Please provide your details");
    }

    #[test]
    fn test_elicit_result_actions() {
        // Test accept result
        let mut content = std::collections::HashMap::new();
        content.insert(
            "name".to_string(),
            serde_json::Value::String("John".to_string()),
        );
        content.insert(
            "age".to_string(),
            serde_json::Value::Number(serde_json::Number::from(30)),
        );

        let accept_result = ElicitResult::accept(content);
        assert_eq!(accept_result.action, ElicitationAction::Accept);
        assert!(accept_result.content.is_some());

        // Test decline result
        let decline_result = ElicitResult::decline();
        assert_eq!(decline_result.action, ElicitationAction::Decline);
        assert!(decline_result.content.is_none());

        // Test cancel result
        let cancel_result = ElicitResult::cancel();
        assert_eq!(cancel_result.action, ElicitationAction::Cancel);
        assert!(cancel_result.content.is_none());
    }

    #[test]
    fn test_elicit_result_serialization_compliance() {
        // Test that serialization matches MCP spec exactly
        let mut content = std::collections::HashMap::new();
        content.insert(
            "field1".to_string(),
            serde_json::Value::String("value1".to_string()),
        );

        let result = ElicitResult::accept(content);
        let json = serde_json::to_string(&result).unwrap();

        // Parse back to ensure it's valid
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Check action field
        assert_eq!(parsed["action"].as_str().unwrap(), "accept");

        // Check content field
        assert!(parsed["content"].is_object());
        assert_eq!(parsed["content"]["field1"].as_str().unwrap(), "value1");
    }

    #[test]
    fn test_primitive_schema_serialization() {
        // Test string schema
        let string_schema = PrimitiveSchemaDefinition::String {
            title: Some("Title".to_string()),
            description: Some("Description".to_string()),
            format: Some("email".to_string()),
            min_length: Some(1),
            max_length: Some(100),
            enum_values: None,
            enum_names: None,
        };

        let json = serde_json::to_string(&string_schema).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["type"].as_str().unwrap(), "string");
        assert_eq!(parsed["format"].as_str().unwrap(), "email");
        assert_eq!(parsed["minLength"].as_u64().unwrap(), 1);
        assert_eq!(parsed["maxLength"].as_u64().unwrap(), 100);

        // Test enum schema
        let enum_schema = PrimitiveSchemaDefinition::String {
            title: None,
            description: Some("Select option".to_string()),
            format: None,
            min_length: None,
            max_length: None,
            enum_values: Some(vec!["option1".to_string(), "option2".to_string()]),
            enum_names: Some(vec!["Option 1".to_string(), "Option 2".to_string()]),
        };

        let enum_json = serde_json::to_string(&enum_schema).unwrap();
        let enum_parsed: serde_json::Value = serde_json::from_str(&enum_json).unwrap();

        assert_eq!(enum_parsed["type"].as_str().unwrap(), "string");
        assert_eq!(enum_parsed["enum"].as_array().unwrap().len(), 2);
        assert_eq!(enum_parsed["enumNames"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_server_request_with_elicitation() {
        let schema = ElicitationSchema::new().add_string_property(
            "email",
            true,
            Some("Your email address".to_string()),
        );

        let request_params = ElicitRequestParams {
            message: "Please provide your email".to_string(),
            requested_schema: schema,
        };

        let server_request = ServerRequest::ElicitationCreate(request_params);

        // Serialize and verify
        let json = serde_json::to_string(&server_request).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["method"].as_str().unwrap(), "elicitation/create");
        assert!(parsed["message"].is_string());
        assert!(parsed["requestedSchema"].is_object());
    }

    #[test]
    fn test_completion_request_serialization() {
        let argument = ArgumentInfo {
            name: "file_path".to_string(),
            value: "/home/user/doc".to_string(),
        };

        let reference = CompletionReference::ResourceTemplate(ResourceTemplateReferenceData {
            uri: "/files/{path}".to_string(),
        });

        let request = CompleteRequest::new(argument, reference);

        // Serialize
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("completion/complete"));
        assert!(json.contains("file_path"));
        assert!(json.contains("/home/user/doc"));

        // Deserialize
        let deserialized: CompleteRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.method, "completion/complete");
        assert_eq!(deserialized.params.argument.name, "file_path");
    }

    #[test]
    fn test_completion_reference_types() {
        // Test prompt reference
        let prompt_ref = CompletionReference::Prompt(PromptReferenceData {
            name: "code_review".to_string(),
            title: Some("Code Review Assistant".to_string()),
        });

        let json = serde_json::to_string(&prompt_ref).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"].as_str().unwrap(), "ref/prompt");
        assert_eq!(parsed["name"].as_str().unwrap(), "code_review");

        // Test resource template reference
        let resource_ref = CompletionReference::ResourceTemplate(ResourceTemplateReferenceData {
            uri: "/api/{endpoint}".to_string(),
        });

        let json = serde_json::to_string(&resource_ref).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"].as_str().unwrap(), "ref/resource");
        assert_eq!(parsed["uri"].as_str().unwrap(), "/api/{endpoint}");
    }

    #[test]
    fn test_completion_response_with_pagination() {
        // Test simple response
        let simple =
            CompletionResponse::new(vec!["file1.txt".to_string(), "file2.txt".to_string()]);
        assert_eq!(simple.values.len(), 2);
        assert!(simple.total.is_none());

        // Test paginated response
        let paginated = CompletionResponse::paginated(
            vec!["item1".to_string(), "item2".to_string()],
            100,
            true,
        );
        assert_eq!(paginated.values.len(), 2);
        assert_eq!(paginated.total, Some(100));
        assert_eq!(paginated.has_more, Some(true));

        // Test serialization
        let json = serde_json::to_string(&paginated).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["total"].as_u64().unwrap(), 100);
        assert!(parsed["hasMore"].as_bool().unwrap());
        assert_eq!(parsed["values"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_complete_result_structure() {
        let completion = CompletionResponse::paginated(
            vec!["option1".to_string(), "option2".to_string()],
            50,
            false,
        );

        let result = CompleteResult::new(completion);

        // Serialize and verify structure
        let json = serde_json::to_string(&result).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Check completion field structure
        assert!(parsed["completion"].is_object());
        assert!(parsed["completion"]["values"].is_array());
        assert_eq!(parsed["completion"]["total"].as_u64().unwrap(), 50);
        assert!(!parsed["completion"]["hasMore"].as_bool().unwrap());

        // Deserialize back
        let deserialized: CompleteResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.completion.values.len(), 2);
        assert_eq!(deserialized.completion.total, Some(50));
    }

    #[test]
    fn test_completion_context() {
        let mut context_args = std::collections::HashMap::new();
        context_args.insert("user_id".to_string(), "12345".to_string());
        context_args.insert("project".to_string(), "main".to_string());

        let context = CompletionContext {
            arguments: Some(context_args),
        };

        let argument = ArgumentInfo {
            name: "endpoint".to_string(),
            value: "api".to_string(),
        };

        let reference = CompletionReference::ResourceTemplate(ResourceTemplateReferenceData {
            uri: "/projects/{project}/endpoints/{endpoint}".to_string(),
        });

        let request = CompleteRequest::new(argument, reference).with_context(context);

        // Verify context is included
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("user_id"));
        assert!(json.contains("12345"));
        assert!(json.contains("project"));
        assert!(json.contains("main"));
    }

    #[test]
    fn test_client_request_with_completion() {
        let argument = ArgumentInfo {
            name: "query".to_string(),
            value: "hello".to_string(),
        };

        let reference = CompletionReference::Prompt(PromptReferenceData {
            name: "greeting".to_string(),
            title: None,
        });

        let complete_params = CompleteRequestParams {
            argument,
            reference,
            context: None,
        };

        let client_request = ClientRequest::Complete(complete_params);

        // Serialize and verify
        let json = serde_json::to_string(&client_request).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["method"].as_str().unwrap(), "completion/complete");
        assert_eq!(parsed["argument"]["name"].as_str().unwrap(), "query");
        assert_eq!(parsed["ref"]["type"].as_str().unwrap(), "ref/prompt");
    }

    #[test]
    fn test_resource_template_creation() {
        // Test basic creation
        let template = ResourceTemplate::new("file_access", "/files/{path}");
        assert_eq!(template.name, "file_access");
        assert_eq!(template.uri_template, "/files/{path}");

        // Test builder pattern
        let enhanced_template = ResourceTemplate::new("api_access", "/api/{endpoint}")
            .with_title("API Access")
            .with_description("Access to REST API endpoints")
            .with_mime_type("application/json");

        assert_eq!(enhanced_template.title, Some("API Access".to_string()));
        assert_eq!(
            enhanced_template.description,
            Some("Access to REST API endpoints".to_string())
        );
        assert_eq!(
            enhanced_template.mime_type,
            Some("application/json".to_string())
        );
    }

    #[test]
    fn test_resource_template_presets() {
        // Test file system template
        let fs_template = ResourceTemplate::file_system("files", "/home/user");
        assert_eq!(fs_template.name, "files");
        assert_eq!(fs_template.uri_template, "file:///home/user/{path}");
        assert_eq!(fs_template.title, Some("File System Access".to_string()));

        // Test API endpoint template
        let api_template = ResourceTemplate::api_endpoint("api", "https://api.example.com");
        assert_eq!(
            api_template.uri_template,
            "https://api.example.com/{endpoint}"
        );
        assert_eq!(api_template.mime_type, Some("application/json".to_string()));

        // Test database query template
        let db_template = ResourceTemplate::database_query("queries");
        assert_eq!(db_template.uri_template, "db://query/{table}?{query*}");
        assert_eq!(db_template.title, Some("Database Query".to_string()));
    }

    #[test]
    fn test_resource_template_serialization() {
        let template = ResourceTemplate::new("test_template", "/test/{id}")
            .with_title("Test Template")
            .with_description("A template for testing")
            .with_mime_type("text/plain");

        let json = serde_json::to_string(&template).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["name"].as_str().unwrap(), "test_template");
        assert_eq!(parsed["uriTemplate"].as_str().unwrap(), "/test/{id}");
        assert_eq!(parsed["title"].as_str().unwrap(), "Test Template");
        assert_eq!(
            parsed["description"].as_str().unwrap(),
            "A template for testing"
        );
        assert_eq!(parsed["mimeType"].as_str().unwrap(), "text/plain");

        // Test deserialization
        let deserialized: ResourceTemplate = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "test_template");
        assert_eq!(deserialized.uri_template, "/test/{id}");
    }

    #[test]
    fn test_list_resource_templates_request() {
        // Test basic request
        let request = ListResourceTemplatesRequest::new();
        assert!(request.params.is_none());

        // Test request with cursor
        let paginated_request = ListResourceTemplatesRequest::with_cursor("cursor123");
        assert!(paginated_request.params.is_some());
        assert_eq!(
            paginated_request.params.unwrap().cursor,
            Some("cursor123".to_string())
        );

        // Test serialization
        let json = serde_json::to_string(&request).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Should serialize to an empty object when no params
        assert!(parsed.as_object().unwrap().is_empty());
    }

    #[test]
    fn test_list_resource_templates_result() {
        let templates = vec![
            ResourceTemplate::new("template1", "/api/{endpoint}"),
            ResourceTemplate::new("template2", "/files/{path}"),
        ];

        // Test basic result
        let result = ListResourceTemplatesResult::new(templates.clone());
        assert_eq!(result.resource_templates.len(), 2);
        assert!(result.next_cursor.is_none());

        // Test paginated result
        let paginated_result =
            ListResourceTemplatesResult::paginated(templates, "next_cursor".to_string());
        assert_eq!(paginated_result.resource_templates.len(), 2);
        assert_eq!(
            paginated_result.next_cursor,
            Some("next_cursor".to_string())
        );

        // Test serialization
        let json = serde_json::to_string(&paginated_result).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(parsed["resourceTemplates"].is_array());
        assert_eq!(parsed["resourceTemplates"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["nextCursor"].as_str().unwrap(), "next_cursor");

        // Test deserialization
        let deserialized: ListResourceTemplatesResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.resource_templates.len(), 2);
        assert_eq!(deserialized.next_cursor, Some("next_cursor".to_string()));
    }

    #[test]
    fn test_client_request_with_resource_templates() {
        let request = ListResourceTemplatesRequest::with_cursor("abc123");
        let client_request = ClientRequest::ListResourceTemplates(request);

        // Serialize and verify
        let json = serde_json::to_string(&client_request).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(
            parsed["method"].as_str().unwrap(),
            "resources/templates/list"
        );
        assert_eq!(parsed["params"]["cursor"].as_str().unwrap(), "abc123");
    }

    #[test]
    fn test_complex_uri_templates() {
        // Test RFC 6570 style templates
        let complex_templates = vec![
            ResourceTemplate::new(
                "github",
                "https://api.github.com/repos/{owner}/{repo}/contents/{+path}",
            ),
            ResourceTemplate::new("search", "/search{?q,type,sort,order}"),
            ResourceTemplate::new("matrix", "/matrix{;x,y}/data"),
            ResourceTemplate::new("fragment", "/documents/{id}{#section}"),
        ];

        for template in complex_templates {
            let json = serde_json::to_string(&template).unwrap();
            let deserialized: ResourceTemplate = serde_json::from_str(&json).unwrap();
            assert_eq!(template.uri_template, deserialized.uri_template);
        }
    }

    #[test]
    fn test_resource_template_with_annotations() {
        let annotations = Annotations {
            priority: Some(1.0),
            ..Default::default()
        };

        let template =
            ResourceTemplate::new("important", "/critical/{id}").with_annotations(annotations);

        assert!(template.annotations.is_some());
        assert_eq!(template.annotations.unwrap().priority, Some(1.0));
    }

    #[test]
    fn test_ping_request_creation() {
        // Test basic ping
        let ping = PingRequest::new();
        assert_eq!(ping.method, "ping");
        assert!(ping.params.is_none());

        // Test ping with metadata
        let meta_value = serde_json::json!({"timestamp": "2025-08-29T12:00:00Z"});
        let ping_with_meta = PingRequest::with_meta(meta_value.clone());
        assert_eq!(ping_with_meta.method, "ping");
        assert!(ping_with_meta.params.is_some());
        assert_eq!(ping_with_meta.params.unwrap()._meta, Some(meta_value));
    }

    #[test]
    fn test_ping_serialization() {
        // Test basic ping serialization
        let ping = PingRequest::new();
        let json = serde_json::to_string(&ping).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["method"].as_str().unwrap(), "ping");

        // Test deserialization
        let deserialized: PingRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.method, "ping");

        // Test ping with metadata
        let meta_value = serde_json::json!({"client": "test", "version": "1.0"});
        let ping_with_meta = PingRequest::with_meta(meta_value.clone());
        let json_with_meta = serde_json::to_string(&ping_with_meta).unwrap();
        let parsed_meta: serde_json::Value = serde_json::from_str(&json_with_meta).unwrap();

        assert!(parsed_meta["params"].is_object());
        assert_eq!(
            parsed_meta["params"]["_meta"]["client"].as_str().unwrap(),
            "test"
        );
    }

    #[test]
    fn test_ping_result() {
        // Test basic result
        let result = PingResult::new();
        assert!(result._meta.is_none());

        // Test result with metadata
        let meta = serde_json::json!({"latency_ms": 42});
        let result_with_meta = PingResult::with_meta(meta.clone());
        assert_eq!(result_with_meta._meta, Some(meta));

        // Test serialization
        let json = serde_json::to_string(&result).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        // Empty result should serialize to empty object
        assert!(parsed.as_object().unwrap().is_empty());

        // Test deserialization
        let deserialized: PingResult = serde_json::from_str(&json).unwrap();
        assert!(deserialized._meta.is_none());
    }

    #[test]
    fn test_ping_params() {
        // Test basic params
        let params = PingParams::new();
        assert!(params._meta.is_none());

        // Test params with metadata
        let meta = serde_json::json!({"timeout": 5000});
        let params_with_meta = PingParams::new().with_meta(meta.clone());
        assert_eq!(params_with_meta._meta, Some(meta));

        // Test serialization
        let json = serde_json::to_string(&params).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.as_object().unwrap().is_empty());
    }

    #[test]
    fn test_server_request_with_ping() {
        let ping_params = PingParams::new();
        let server_request = ServerRequest::Ping(ping_params);

        // Serialize and verify
        let json = serde_json::to_string(&server_request).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["method"].as_str().unwrap(), "ping");
    }

    #[test]
    fn test_client_request_with_ping() {
        let ping_params = PingParams::new();
        let client_request = ClientRequest::Ping(ping_params);

        // Serialize and verify
        let json = serde_json::to_string(&client_request).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["method"].as_str().unwrap(), "ping");
    }

    #[test]
    fn test_ping_protocol_bidirectional() {
        // Test that ping can be used from both client and server
        let meta = serde_json::json!({"source": "test"});

        // Client-initiated ping
        let client_ping = ClientRequest::Ping(PingParams::new().with_meta(meta.clone()));
        let client_json = serde_json::to_string(&client_ping).unwrap();

        // Server-initiated ping
        let server_ping = ServerRequest::Ping(PingParams::new().with_meta(meta.clone()));
        let server_json = serde_json::to_string(&server_ping).unwrap();

        // Both should have same structure
        let client_parsed: serde_json::Value = serde_json::from_str(&client_json).unwrap();
        let server_parsed: serde_json::Value = serde_json::from_str(&server_json).unwrap();

        assert_eq!(client_parsed["method"], server_parsed["method"]);
        assert_eq!(
            client_parsed["_meta"]["source"],
            server_parsed["_meta"]["source"]
        );
    }
}
