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

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::core::{Annotations, Base64String, MimeType, Uri};

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
