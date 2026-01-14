//! Content types for MCP messages.
//!
//! This module defines the content types used in MCP protocol messages,
//! including text, images, audio, and embedded resources.

use serde::{Deserialize, Serialize};

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

/// Content block in MCP messages.
///
/// This enum represents all possible content types in the MCP protocol:
/// - Text content with optional annotations
/// - Image content (base64 encoded)
/// - Audio content (base64 encoded)
/// - Embedded resource content
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Content {
    /// Text content
    Text(TextContent),
    /// Image content (base64 encoded)
    Image(ImageContent),
    /// Audio content (base64 encoded)
    Audio(AudioContent),
    /// Embedded resource content
    Resource(EmbeddedResource),
}

impl Default for Content {
    fn default() -> Self {
        Self::text("")
    }
}

impl Content {
    /// Create text content.
    ///
    /// # Example
    /// ```
    /// use turbomcp_types::Content;
    ///
    /// let content = Content::text("Hello, world!");
    /// assert_eq!(content.as_text(), Some("Hello, world!"));
    /// ```
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(TextContent {
            text: text.into(),
            annotations: None,
        })
    }

    /// Create image content from base64 data.
    ///
    /// # Example
    /// ```
    /// use turbomcp_types::Content;
    ///
    /// let content = Content::image("base64data...", "image/png");
    /// ```
    #[must_use]
    pub fn image(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self::Image(ImageContent {
            data: data.into(),
            mime_type: mime_type.into(),
            annotations: None,
        })
    }

    /// Create audio content from base64 data.
    ///
    /// # Example
    /// ```
    /// use turbomcp_types::Content;
    ///
    /// let content = Content::audio("base64data...", "audio/mp3");
    /// ```
    #[must_use]
    pub fn audio(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self::Audio(AudioContent {
            data: data.into(),
            mime_type: mime_type.into(),
            annotations: None,
        })
    }

    /// Create embedded resource content.
    ///
    /// # Example
    /// ```
    /// use turbomcp_types::Content;
    ///
    /// let content = Content::resource("file:///example.txt", "Hello!");
    /// ```
    #[must_use]
    pub fn resource(uri: impl Into<String>, text: impl Into<String>) -> Self {
        Self::Resource(EmbeddedResource {
            uri: uri.into(),
            mime_type: Some("text/plain".into()),
            text: Some(text.into()),
            blob: None,
            annotations: None,
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
            Self::Resource(r) => r.annotations = Some(annotations),
        }
        self
    }
}

/// Text content with optional annotations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextContent {
    /// The text content
    pub text: String,
    /// Optional annotations (audience, priority, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
}

impl TextContent {
    /// Create new text content.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            annotations: None,
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
}

/// Embedded resource content in a message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmbeddedResource {
    /// Resource URI
    pub uri: String,
    /// MIME type of the resource
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Text content (for text resources)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Binary content as base64 (for binary resources)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
    /// Optional annotations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
}

/// Annotations for content providing metadata.
///
/// Per MCP 2025-11-25, annotations indicate:
/// - Who should see the content (audience)
/// - Relative importance (priority)
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Annotations {
    /// Target audience for this content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<Vec<Role>>,
    /// Priority level (higher = more important)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<f64>,
}

impl Annotations {
    /// Create annotations for user audience only.
    #[must_use]
    pub fn for_user() -> Self {
        Self {
            audience: Some(vec![Role::User]),
            priority: None,
        }
    }

    /// Create annotations for assistant audience only.
    #[must_use]
    pub fn for_assistant() -> Self {
        Self {
            audience: Some(vec![Role::Assistant]),
            priority: None,
        }
    }

    /// Set the priority level.
    #[must_use]
    pub fn with_priority(mut self, priority: f64) -> Self {
        self.priority = Some(priority);
        self
    }
}

/// A message in a prompt or conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    /// The role of this message (user or assistant)
    pub role: Role,
    /// The content of this message
    pub content: Content,
}

impl Message {
    /// Create a new message.
    #[must_use]
    pub fn new(role: Role, content: Content) -> Self {
        Self { role, content }
    }

    /// Create a user message with text content.
    ///
    /// # Example
    /// ```
    /// use turbomcp_types::Message;
    ///
    /// let msg = Message::user("Hello!");
    /// assert_eq!(msg.content.as_text(), Some("Hello!"));
    /// ```
    #[must_use]
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: Content::text(text),
        }
    }

    /// Create an assistant message with text content.
    ///
    /// # Example
    /// ```
    /// use turbomcp_types::Message;
    ///
    /// let msg = Message::assistant("I can help with that!");
    /// ```
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
}
