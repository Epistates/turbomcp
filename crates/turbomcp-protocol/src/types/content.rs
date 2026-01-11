//! Message content types
//!
//! This module contains all content block types used in MCP messages.
//! Content blocks allow rich message composition with text, images, audio,
//! and resource references.
//!
//! # Content Types
//!
//! - [`ContentBlock`] - Content block enum (text, image, audio, resource link, embedded resource)
//! - [`TextContent`] - Plain text content with annotations
//! - [`ImageContent`] - Base64-encoded image content
//! - [`AudioContent`] - Base64-encoded audio content
//! - [`ResourceLink`] - Reference to external resource
//! - [`EmbeddedResource`] - Embedded resource content
//! - [`ContentType`] - Content type enumeration (JSON/Binary/Text)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::core::{Annotations, Base64String, MimeType, Uri};

/// Content type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentType {
    /// JSON content
    Json,
    /// Binary content
    Binary,
    /// Plain text content
    Text,
}

/// Content block union type
///
/// - MCP 2025-06-18: text, image, audio, resource_link, resource
/// - MCP 2025-11-25 draft (SEP-1577): + tool_use, tool_result
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
    /// Tool use (MCP 2025-11-25 draft, SEP-1577)
    #[serde(rename = "tool_use")]
    ToolUse(ToolUseContent),
    /// Tool result (MCP 2025-11-25 draft, SEP-1577)
    #[serde(rename = "tool_result")]
    ToolResult(ToolResultContent),
}

/// Backward compatibility alias for `ContentBlock`.
///
/// The MCP specification originally named this type `Content`, but later renamed it to
/// `ContentBlock` for clarity. This alias exists to maintain backward compatibility with
/// code written against earlier versions of the TurboMCP SDK.
///
/// **For new code**, prefer using `ContentBlock` directly as it matches the current
/// MCP specification terminology.
///
/// # Example
///
/// ```rust
/// use turbomcp_protocol::types::{Content, ContentBlock, TextContent};
///
/// // Both are equivalent:
/// let content_old: Content = ContentBlock::Text(TextContent {
///     text: "Hello".to_string(),
///     annotations: None,
///     meta: None,
/// });
///
/// let content_new: ContentBlock = ContentBlock::Text(TextContent {
///     text: "Hello".to_string(),
///     annotations: None,
///     meta: None,
/// });
/// ```
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
    pub data: Base64String,
    /// The MIME type of the image. Different providers may support different image types
    #[serde(rename = "mimeType")]
    pub mime_type: MimeType,
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
    pub data: Base64String,
    /// The MIME type of the audio. Different providers may support different audio types
    #[serde(rename = "mimeType")]
    pub mime_type: MimeType,
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
    pub uri: Uri,
    /// A description of what this resource represents
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The MIME type of this resource, if known
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<MimeType>,
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

/// Text resource contents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextResourceContents {
    /// The URI of this resource
    pub uri: Uri,
    /// The MIME type of this resource, if known
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<MimeType>,
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
    pub uri: Uri,
    /// The MIME type of this resource, if known
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<MimeType>,
    /// Base64-encoded binary data
    pub blob: Base64String,
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

/// Tool use content (MCP 2025-11-25 draft, SEP-1577)
///
/// Represents a request from the LLM to call a tool during sampling.
/// The model wants to execute a function and receive its results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUseContent {
    /// A unique identifier for this tool use
    /// This ID is used to match tool results to their corresponding tool uses
    pub id: String,

    /// The name of the tool to call
    pub name: String,

    /// The arguments to pass to the tool, conforming to the tool's input schema
    pub input: serde_json::Value,

    /// Optional metadata about the tool use
    /// Clients SHOULD preserve this field when including tool uses in subsequent
    /// sampling requests to enable caching optimizations
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, serde_json::Value>>,
}

/// Tool result content (MCP 2025-11-25 draft, SEP-1577)
///
/// Represents the result of executing a tool that was requested by the LLM.
/// The server provides the tool execution results back to the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultContent {
    /// The ID of the tool use this result corresponds to
    /// This MUST match the ID from a previous ToolUseContent
    #[serde(rename = "toolUseId")]
    pub tool_use_id: String,

    /// The unstructured result content of the tool use
    /// Can include text, images, audio, resource links, and embedded resources
    pub content: Vec<ContentBlock>,

    /// An optional structured result object
    /// If the tool defined an outputSchema, this SHOULD conform to that schema
    #[serde(rename = "structuredContent", skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<serde_json::Value>,

    /// Whether the tool use resulted in an error
    /// If true, the content typically describes the error that occurred
    /// Default: false
    #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,

    /// Optional metadata about the tool result
    /// Clients SHOULD preserve this field when including tool results in subsequent
    /// sampling requests to enable caching optimizations
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, serde_json::Value>>,
}
