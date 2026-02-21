//! Content types for MCP messages (MCP 2025-11-25).
//!
//! This module defines the content type unions used in the MCP protocol:
//!
//! - [`Content`] (`ContentBlock`): Used in tool call results and prompt messages.
//!   Variants: Text, Image, Audio, ResourceLink, Resource (EmbeddedResource).
//!
//! - [`SamplingContent`] (`SamplingMessageContentBlock`): Used in sampling messages.
//!   Variants: Text, Image, Audio, ToolUse, ToolResult.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Role in a conversation or prompt message.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// User role (human or client)
    #[default]
    User,
    /// Assistant role (AI or server)
    Assistant,
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::User => f.write_str("user"),
            Self::Assistant => f.write_str("assistant"),
        }
    }
}

// =============================================================================
// ContentBlock — used in tool results and prompt messages
// =============================================================================

/// Content block in MCP messages (`ContentBlock` per spec).
///
/// Used in `CallToolResult.content` and `PromptMessage.content`.
///
/// Per MCP 2025-11-25, the union is:
/// `TextContent | ImageContent | AudioContent | ResourceLink | EmbeddedResource`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum Content {
    /// Text content
    #[serde(rename = "text")]
    Text(TextContent),
    /// Image content (base64 encoded)
    #[serde(rename = "image")]
    Image(ImageContent),
    /// Audio content (base64 encoded)
    #[serde(rename = "audio")]
    Audio(AudioContent),
    /// Resource link (reference to a resource without embedding)
    #[serde(rename = "resource_link")]
    ResourceLink(ResourceLink),
    /// Embedded resource content
    #[serde(rename = "resource")]
    Resource(EmbeddedResource),
}

impl Default for Content {
    fn default() -> Self {
        Self::text("")
    }
}

impl Content {
    /// Create text content.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(TextContent {
            text: text.into(),
            annotations: None,
            meta: None,
        })
    }

    /// Create image content from base64 data.
    #[must_use]
    pub fn image(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self::Image(ImageContent {
            data: data.into(),
            mime_type: mime_type.into(),
            annotations: None,
            meta: None,
        })
    }

    /// Create audio content from base64 data.
    #[must_use]
    pub fn audio(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self::Audio(AudioContent {
            data: data.into(),
            mime_type: mime_type.into(),
            annotations: None,
            meta: None,
        })
    }

    /// Create a resource link.
    #[must_use]
    pub fn resource_link(resource: crate::definitions::Resource) -> Self {
        Self::ResourceLink(ResourceLink {
            uri: resource.uri,
            name: resource.name,
            description: resource.description,
            title: resource.title,
            icons: resource.icons,
            mime_type: resource.mime_type,
            annotations: resource.annotations,
            size: resource.size,
            meta: resource.meta,
        })
    }

    /// Create embedded resource content.
    #[must_use]
    pub fn resource(uri: impl Into<String>, text: impl Into<String>) -> Self {
        Self::Resource(EmbeddedResource {
            resource: ResourceContents::Text(TextResourceContents {
                uri: uri.into(),
                mime_type: Some("text/plain".into()),
                text: text.into(),
                meta: None,
            }),
            annotations: None,
            meta: None,
        })
    }

    /// Check if this is text content.
    #[must_use]
    pub fn is_text(&self) -> bool {
        matches!(self, Self::Text(_))
    }

    /// Get the text if this is text content.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(t) => Some(&t.text),
            _ => None,
        }
    }

    /// Check if this is image content.
    #[must_use]
    pub fn is_image(&self) -> bool {
        matches!(self, Self::Image(_))
    }

    /// Check if this is audio content.
    #[must_use]
    pub fn is_audio(&self) -> bool {
        matches!(self, Self::Audio(_))
    }

    /// Check if this is a resource link.
    #[must_use]
    pub fn is_resource_link(&self) -> bool {
        matches!(self, Self::ResourceLink(_))
    }

    /// Check if this is resource content.
    #[must_use]
    pub fn is_resource(&self) -> bool {
        matches!(self, Self::Resource(_))
    }

    /// Add annotations to this content.
    #[must_use]
    pub fn with_annotations(mut self, annotations: Annotations) -> Self {
        match &mut self {
            Self::Text(t) => t.annotations = Some(annotations),
            Self::Image(i) => i.annotations = Some(annotations),
            Self::Audio(a) => a.annotations = Some(annotations),
            Self::ResourceLink(r) => {
                r.annotations = Some(crate::definitions::ResourceAnnotations {
                    audience: annotations.audience,
                    priority: annotations.priority,
                    last_modified: annotations.last_modified,
                })
            }
            Self::Resource(r) => r.annotations = Some(annotations),
        }
        self
    }
}

// =============================================================================
// SamplingMessageContentBlock — used in sampling messages
// =============================================================================

/// Content block for sampling messages (`SamplingMessageContentBlock` per spec).
///
/// Per MCP 2025-11-25, the union is:
/// `TextContent | ImageContent | AudioContent | ToolUseContent | ToolResultContent`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum SamplingContent {
    /// Text content
    #[serde(rename = "text")]
    Text(TextContent),
    /// Image content (base64 encoded)
    #[serde(rename = "image")]
    Image(ImageContent),
    /// Audio content (base64 encoded)
    #[serde(rename = "audio")]
    Audio(AudioContent),
    /// Tool use content (assistant requesting tool invocation)
    #[serde(rename = "tool_use")]
    ToolUse(ToolUseContent),
    /// Tool result content (result of a tool invocation)
    #[serde(rename = "tool_result")]
    ToolResult(ToolResultContent),
}

impl Default for SamplingContent {
    fn default() -> Self {
        Self::text("")
    }
}

impl SamplingContent {
    /// Create text content.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(TextContent {
            text: text.into(),
            annotations: None,
            meta: None,
        })
    }

    /// Get the text if this is text content.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(t) => Some(&t.text),
            _ => None,
        }
    }
}

/// Wrapper that deserializes as either a single content block or an array.
///
/// Per MCP 2025-11-25, `SamplingMessage.content` is
/// `SamplingMessageContentBlock | SamplingMessageContentBlock[]`.
///
/// `Serialize` and `Deserialize` are implemented manually below to handle
/// the single-vs-array polymorphism (single serializes as object, array as array).
#[derive(Debug, Clone, PartialEq)]
pub enum SamplingContentBlock {
    /// A single content block.
    Single(SamplingContent),
    /// Multiple content blocks.
    Multiple(Vec<SamplingContent>),
}

impl Default for SamplingContentBlock {
    fn default() -> Self {
        Self::Single(SamplingContent::default())
    }
}

impl SamplingContentBlock {
    /// Get the text of the first text content block, if any.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Single(c) => c.as_text(),
            Self::Multiple(v) => v.iter().find_map(|c| c.as_text()),
        }
    }

    /// Collect all content blocks into a `Vec` of references.
    ///
    /// Note: this allocates. For iteration, use `iter()` instead.
    #[must_use]
    pub fn to_vec(&self) -> Vec<&SamplingContent> {
        match self {
            Self::Single(c) => vec![c],
            Self::Multiple(v) => v.iter().collect(),
        }
    }
}

impl Serialize for SamplingContentBlock {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Single(c) => c.serialize(serializer),
            Self::Multiple(v) => v.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for SamplingContentBlock {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        if value.is_array() {
            let v: Vec<SamplingContent> =
                serde_json::from_value(value).map_err(serde::de::Error::custom)?;
            Ok(Self::Multiple(v))
        } else {
            let c: SamplingContent =
                serde_json::from_value(value).map_err(serde::de::Error::custom)?;
            Ok(Self::Single(c))
        }
    }
}

impl From<SamplingContent> for SamplingContentBlock {
    fn from(c: SamplingContent) -> Self {
        Self::Single(c)
    }
}

impl From<Vec<SamplingContent>> for SamplingContentBlock {
    fn from(v: Vec<SamplingContent>) -> Self {
        Self::Multiple(v)
    }
}

// =============================================================================
// Individual content types
// =============================================================================

/// Text content with optional annotations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextContent {
    /// The text content
    pub text: String,
    /// Optional annotations (audience, priority, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
    /// Extension metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

impl TextContent {
    /// Create new text content.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            annotations: None,
            meta: None,
        }
    }
}

/// Image content (base64 encoded).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageContent {
    /// Base64-encoded image data
    pub data: String,
    /// MIME type of the image
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    /// Optional annotations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
    /// Extension metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

/// Audio content (base64 encoded).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AudioContent {
    /// Base64-encoded audio data
    pub data: String,
    /// MIME type of the audio
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    /// Optional annotations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
    /// Extension metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

/// Tool use content in a sampling message (assistant requesting tool invocation).
///
/// New in MCP 2025-11-25. Part of `SamplingMessageContentBlock`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolUseContent {
    /// Unique ID for this tool use.
    pub id: String,
    /// Name of the tool to invoke.
    pub name: String,
    /// Input arguments for the tool.
    pub input: HashMap<String, Value>,
    /// Extension metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

/// Tool result content in a sampling message (result of a tool invocation).
///
/// New in MCP 2025-11-25. Part of `SamplingMessageContentBlock`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolResultContent {
    /// ID of the tool use this result corresponds to.
    #[serde(rename = "toolUseId")]
    pub tool_use_id: String,
    /// Content blocks from the tool result.
    pub content: Vec<Content>,
    /// Structured content conforming to the tool's output schema.
    #[serde(rename = "structuredContent", skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
    /// Whether the tool execution resulted in an error.
    #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    /// Extension metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

/// A resource link (reference without embedding contents).
///
/// New in MCP 2025-11-25. Extends `Resource` with `type: "resource_link"`.
/// Resource links returned by tools are not guaranteed to appear in `resources/list`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceLink {
    /// Resource URI
    pub uri: String,
    /// Resource name
    pub name: String,
    /// Resource description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Human-readable title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Resource icons
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icons: Option<Vec<crate::definitions::Icon>>,
    /// MIME type of the resource content
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Resource annotations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<crate::definitions::ResourceAnnotations>,
    /// Size in bytes (if known)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Extension metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<std::collections::HashMap<String, Value>>,
}

/// Embedded resource content in a message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmbeddedResource {
    /// The actual resource contents
    pub resource: ResourceContents,
    /// Optional annotations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
    /// Extension metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

// =============================================================================
// Resource contents
// =============================================================================

/// Contents of a resource (text or binary).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ResourceContents {
    /// Text resource content
    Text(TextResourceContents),
    /// Binary resource content
    Blob(BlobResourceContents),
}

impl ResourceContents {
    /// Get the URI of this resource content.
    #[must_use]
    pub fn uri(&self) -> &str {
        match self {
            Self::Text(t) => &t.uri,
            Self::Blob(b) => &b.uri,
        }
    }

    /// Get the text content, if this is a text resource.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text(t) => Some(&t.text),
            Self::Blob(_) => None,
        }
    }

    /// Get the blob (base64) content, if this is a binary resource.
    #[must_use]
    pub fn blob(&self) -> Option<&str> {
        match self {
            Self::Text(_) => None,
            Self::Blob(b) => Some(&b.blob),
        }
    }

    /// Get the MIME type, if set.
    #[must_use]
    pub fn mime_type(&self) -> Option<&str> {
        match self {
            Self::Text(t) => t.mime_type.as_deref(),
            Self::Blob(b) => b.mime_type.as_deref(),
        }
    }
}

/// Textual resource contents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextResourceContents {
    /// Resource URI
    pub uri: String,
    /// MIME type
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Text content
    pub text: String,
    /// Extension metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

/// Binary resource contents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BlobResourceContents {
    /// Resource URI
    pub uri: String,
    /// MIME type
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Base64-encoded binary data
    pub blob: String,
    /// Extension metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

// =============================================================================
// Annotations
// =============================================================================

/// Annotations for content providing metadata.
///
/// Per MCP 2025-11-25, annotations indicate:
/// - Who should see the content (audience)
/// - Relative importance (priority)
/// - When it was last modified
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Annotations {
    /// Target audience for this content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<Vec<Role>>,
    /// Priority level (0.0 to 1.0, higher = more important)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<f64>,
    /// Last modified timestamp (ISO 8601)
    #[serde(rename = "lastModified", skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
}

impl Annotations {
    /// Create annotations for user audience only.
    #[must_use]
    pub fn for_user() -> Self {
        Self {
            audience: Some(vec![Role::User]),
            ..Default::default()
        }
    }

    /// Create annotations for assistant audience only.
    #[must_use]
    pub fn for_assistant() -> Self {
        Self {
            audience: Some(vec![Role::Assistant]),
            ..Default::default()
        }
    }

    /// Set the priority level.
    #[must_use]
    pub fn with_priority(mut self, priority: f64) -> Self {
        self.priority = Some(priority);
        self
    }

    /// Set the last modified timestamp.
    #[must_use]
    pub fn with_last_modified(mut self, timestamp: impl Into<String>) -> Self {
        self.last_modified = Some(timestamp.into());
        self
    }
}

// =============================================================================
// Message (PromptMessage per spec)
// =============================================================================

/// A message in a prompt (`PromptMessage` per spec).
///
/// Per MCP 2025-11-25, `content` is a single `ContentBlock` (not an array).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    /// The role of this message (user or assistant)
    pub role: Role,
    /// The content of this message (single ContentBlock)
    pub content: Content,
}

impl Message {
    /// Create a new message.
    #[must_use]
    pub fn new(role: Role, content: Content) -> Self {
        Self { role, content }
    }

    /// Create a user message with text content.
    #[must_use]
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: Content::text(text),
        }
    }

    /// Create an assistant message with text content.
    #[must_use]
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: Content::text(text),
        }
    }

    /// Check if this is a user message.
    #[must_use]
    pub fn is_user(&self) -> bool {
        self.role == Role::User
    }

    /// Check if this is an assistant message.
    #[must_use]
    pub fn is_assistant(&self) -> bool {
        self.role == Role::Assistant
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_text() {
        let content = Content::text("Hello");
        assert!(content.is_text());
        assert_eq!(content.as_text(), Some("Hello"));
    }

    #[test]
    fn test_content_image() {
        let content = Content::image("base64data", "image/png");
        assert!(content.is_image());
        assert!(!content.is_text());
    }

    #[test]
    fn test_content_serde() {
        let content = Content::text("Hello");
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("\"text\":\"Hello\""));
    }

    #[test]
    fn test_resource_link_serde() {
        let link = Content::ResourceLink(ResourceLink {
            uri: "file:///test.txt".into(),
            name: "test".into(),
            description: None,
            title: None,
            icons: None,
            mime_type: Some("text/plain".into()),
            annotations: None,
            size: None,
            meta: None,
        });
        let json = serde_json::to_string(&link).unwrap();
        assert!(json.contains("\"type\":\"resource_link\""));
        assert!(json.contains("\"uri\":\"file:///test.txt\""));

        // Round-trip
        let parsed: Content = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_resource_link());
    }

    #[test]
    fn test_sampling_content_tool_use_serde() {
        let content = SamplingContent::ToolUse(ToolUseContent {
            id: "tu_1".into(),
            name: "search".into(),
            input: [("query".to_string(), Value::String("test".into()))].into(),
            meta: None,
        });
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("\"type\":\"tool_use\""));
        assert!(json.contains("\"id\":\"tu_1\""));

        let parsed: SamplingContent = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, SamplingContent::ToolUse(_)));
    }

    #[test]
    fn test_sampling_content_block_single() {
        let block = SamplingContentBlock::Single(SamplingContent::text("hello"));
        let json = serde_json::to_string(&block).unwrap();
        // Single should serialize as an object, not array
        assert!(json.starts_with('{'));
        let parsed: SamplingContentBlock = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, SamplingContentBlock::Single(_)));
    }

    #[test]
    fn test_sampling_content_block_multiple() {
        let block = SamplingContentBlock::Multiple(vec![
            SamplingContent::text("hello"),
            SamplingContent::text("world"),
        ]);
        let json = serde_json::to_string(&block).unwrap();
        // Multiple should serialize as an array
        assert!(json.starts_with('['));
        let parsed: SamplingContentBlock = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, SamplingContentBlock::Multiple(v) if v.len() == 2));
    }

    #[test]
    fn test_message_user() {
        let msg = Message::user("Hello");
        assert!(msg.is_user());
        assert!(!msg.is_assistant());
    }

    #[test]
    fn test_message_assistant() {
        let msg = Message::assistant("Hi there");
        assert!(msg.is_assistant());
        assert!(!msg.is_user());
    }

    #[test]
    fn test_annotations_for_user() {
        let ann = Annotations::for_user().with_priority(1.0);
        assert_eq!(ann.audience, Some(vec![Role::User]));
        assert_eq!(ann.priority, Some(1.0));
    }

    #[test]
    fn test_content_with_annotations() {
        let content = Content::text("Hello").with_annotations(Annotations::for_user());
        if let Content::Text(t) = content {
            assert!(t.annotations.is_some());
        } else {
            panic!("Expected text content");
        }
    }

    // C-1: ResourceContents untagged deserialization disambiguation
    #[test]
    fn test_resource_contents_text_deser() {
        let json = r#"{"uri":"file:///test.txt","mimeType":"text/plain","text":"hello"}"#;
        let rc: ResourceContents = serde_json::from_str(json).unwrap();
        assert!(matches!(rc, ResourceContents::Text(_)));
        assert_eq!(rc.uri(), "file:///test.txt");
        assert_eq!(rc.text(), Some("hello"));
        assert!(rc.blob().is_none());
    }

    #[test]
    fn test_resource_contents_blob_deser() {
        let json = r#"{"uri":"file:///img.png","mimeType":"image/png","blob":"aGVsbG8="}"#;
        let rc: ResourceContents = serde_json::from_str(json).unwrap();
        assert!(matches!(rc, ResourceContents::Blob(_)));
        assert_eq!(rc.uri(), "file:///img.png");
        assert_eq!(rc.blob(), Some("aGVsbG8="));
        assert!(rc.text().is_none());
    }

    #[test]
    fn test_resource_contents_round_trip() {
        let text = ResourceContents::Text(TextResourceContents {
            uri: "file:///a.txt".into(),
            mime_type: Some("text/plain".into()),
            text: "content".into(),
            meta: None,
        });
        let json = serde_json::to_string(&text).unwrap();
        let parsed: ResourceContents = serde_json::from_str(&json).unwrap();
        assert_eq!(text, parsed);

        let blob = ResourceContents::Blob(BlobResourceContents {
            uri: "file:///b.bin".into(),
            mime_type: Some("application/octet-stream".into()),
            blob: "AQID".into(),
            meta: None,
        });
        let json = serde_json::to_string(&blob).unwrap();
        let parsed: ResourceContents = serde_json::from_str(&json).unwrap();
        assert_eq!(blob, parsed);
    }

    // C-2: ToolResultContent serde round-trip
    #[test]
    fn test_sampling_content_tool_result_serde() {
        let content = SamplingContent::ToolResult(ToolResultContent {
            tool_use_id: "tu_1".into(),
            content: vec![Content::text("result data")],
            structured_content: Some(serde_json::json!({"key": "value"})),
            is_error: Some(false),
            meta: None,
        });
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("\"type\":\"tool_result\""));
        assert!(json.contains("\"toolUseId\":\"tu_1\""));
        assert!(json.contains("\"structuredContent\""));

        let parsed: SamplingContent = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, SamplingContent::ToolResult(_)));
    }

    // H-1: SamplingContentBlock empty array
    #[test]
    fn test_sampling_content_block_empty_array() {
        let parsed: SamplingContentBlock = serde_json::from_str("[]").unwrap();
        assert!(matches!(parsed, SamplingContentBlock::Multiple(v) if v.is_empty()));
    }

    // H-2: Single-element array vs single object
    #[test]
    fn test_sampling_content_block_single_element_array() {
        let single_obj = r#"{"type":"text","text":"x"}"#;
        let single_arr = r#"[{"type":"text","text":"x"}]"#;

        let parsed_obj: SamplingContentBlock = serde_json::from_str(single_obj).unwrap();
        assert!(matches!(parsed_obj, SamplingContentBlock::Single(_)));

        let parsed_arr: SamplingContentBlock = serde_json::from_str(single_arr).unwrap();
        assert!(matches!(parsed_arr, SamplingContentBlock::Multiple(v) if v.len() == 1));
    }

    // H-3: All Content type discriminants round-trip
    #[test]
    fn test_content_all_type_discriminants() {
        let variants: Vec<(&str, Content)> = vec![
            ("text", Content::text("hi")),
            ("image", Content::image("data", "image/png")),
            ("audio", Content::audio("data", "audio/wav")),
            (
                "resource_link",
                Content::ResourceLink(ResourceLink {
                    uri: "file:///x".into(),
                    name: "x".into(),
                    description: None,
                    title: None,
                    icons: None,
                    mime_type: None,
                    annotations: None,
                    size: None,
                    meta: None,
                }),
            ),
            ("resource", Content::resource("file:///x", "text")),
        ];

        for (expected_type, content) in variants {
            let json = serde_json::to_string(&content).unwrap();
            assert!(
                json.contains(&format!("\"type\":\"{}\"", expected_type)),
                "Missing type discriminant for {expected_type}: {json}"
            );
            let parsed: Content = serde_json::from_str(&json).unwrap();
            assert_eq!(content, parsed, "Round-trip failed for {expected_type}");
        }
    }

    // H-4: _meta field serialization presence/absence
    #[test]
    fn test_meta_field_skip_serializing_if_none() {
        let content = TextContent::new("hello");
        let json = serde_json::to_string(&content).unwrap();
        assert!(!json.contains("_meta"), "None meta should be omitted");

        let mut meta = HashMap::new();
        meta.insert("key".into(), Value::String("val".into()));
        let content = TextContent {
            text: "hello".into(),
            annotations: None,
            meta: Some(meta),
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("\"_meta\""), "Some meta should be present");
    }

    // H-4: _meta on ResourceContents
    #[test]
    fn test_resource_contents_meta_field() {
        let mut meta = HashMap::new();
        meta.insert("k".into(), Value::Bool(true));
        let rc = ResourceContents::Text(TextResourceContents {
            uri: "x".into(),
            mime_type: None,
            text: "y".into(),
            meta: Some(meta),
        });
        let json = serde_json::to_string(&rc).unwrap();
        assert!(json.contains("\"_meta\""));
        let parsed: ResourceContents = serde_json::from_str(&json).unwrap();
        assert_eq!(rc, parsed);
    }

    // H-6: SamplingContentBlock to_vec
    #[test]
    fn test_sampling_content_block_to_vec() {
        let single = SamplingContentBlock::Single(SamplingContent::text("a"));
        assert_eq!(single.to_vec().len(), 1);

        let multi = SamplingContentBlock::Multiple(vec![
            SamplingContent::text("a"),
            SamplingContent::text("b"),
        ]);
        assert_eq!(multi.to_vec().len(), 2);

        // as_text on Multiple finds first text
        assert_eq!(multi.as_text(), Some("a"));
    }
}
