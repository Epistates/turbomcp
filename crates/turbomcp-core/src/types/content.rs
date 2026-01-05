//! Content types for MCP messages.

use alloc::string::String;
use serde::{Deserialize, Serialize};

use super::core::{Annotations, MimeType, Role, Uri};

/// Content types in MCP messages
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Content {
    /// Text content
    Text {
        /// Text content
        text: String,
        /// Optional annotations
        #[serde(skip_serializing_if = "Option::is_none")]
        annotations: Option<Annotations>,
    },
    /// Image content
    Image {
        /// Base64-encoded image data
        data: String,
        /// MIME type
        #[serde(rename = "mimeType")]
        mime_type: MimeType,
        /// Optional annotations
        #[serde(skip_serializing_if = "Option::is_none")]
        annotations: Option<Annotations>,
    },
    /// Audio content
    Audio {
        /// Base64-encoded audio data
        data: String,
        /// MIME type
        #[serde(rename = "mimeType")]
        mime_type: MimeType,
        /// Optional annotations
        #[serde(skip_serializing_if = "Option::is_none")]
        annotations: Option<Annotations>,
    },
    /// Resource reference
    Resource {
        /// Resource content
        resource: ResourceContent,
        /// Optional annotations
        #[serde(skip_serializing_if = "Option::is_none")]
        annotations: Option<Annotations>,
    },
}

impl Content {
    /// Create text content
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text {
            text: text.into(),
            annotations: None,
        }
    }

    /// Create image content
    #[must_use]
    pub fn image(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self::Image {
            data: data.into(),
            mime_type: mime_type.into(),
            annotations: None,
        }
    }

    /// Create audio content
    #[must_use]
    pub fn audio(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self::Audio {
            data: data.into(),
            mime_type: mime_type.into(),
            annotations: None,
        }
    }

    /// Check if this is text content
    #[must_use]
    pub fn is_text(&self) -> bool {
        matches!(self, Self::Text { .. })
    }

    /// Get text if this is text content
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { text, .. } => Some(text),
            _ => None,
        }
    }
}

impl Default for Content {
    fn default() -> Self {
        Self::text("")
    }
}

/// Resource content in messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceContent {
    /// Resource URI
    pub uri: Uri,
    /// MIME type
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<MimeType>,
    /// Text content (for text resources)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Binary content (for binary resources)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
}

/// Sampling message for LLM requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplingMessage {
    /// Message role
    pub role: Role,
    /// Message content
    pub content: Content,
}

impl SamplingMessage {
    /// Create a user message
    #[must_use]
    pub fn user(content: Content) -> Self {
        Self {
            role: Role::User,
            content,
        }
    }

    /// Create an assistant message
    #[must_use]
    pub fn assistant(content: Content) -> Self {
        Self {
            role: Role::Assistant,
            content,
        }
    }

    /// Create a user text message
    #[must_use]
    pub fn user_text(text: impl Into<String>) -> Self {
        Self::user(Content::text(text))
    }

    /// Create an assistant text message
    #[must_use]
    pub fn assistant_text(text: impl Into<String>) -> Self {
        Self::assistant(Content::text(text))
    }
}

/// Prompt message content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptMessage {
    /// Message role
    pub role: Role,
    /// Message content
    pub content: Content,
}

impl PromptMessage {
    /// Create a user prompt message
    #[must_use]
    pub fn user(content: Content) -> Self {
        Self {
            role: Role::User,
            content,
        }
    }

    /// Create an assistant prompt message
    #[must_use]
    pub fn assistant(content: Content) -> Self {
        Self {
            role: Role::Assistant,
            content,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_text() {
        let content = Content::text("Hello, world!");
        assert!(content.is_text());
        assert_eq!(content.as_text(), Some("Hello, world!"));
    }

    #[test]
    fn test_sampling_message() {
        let msg = SamplingMessage::user_text("Hello");
        assert_eq!(msg.role, Role::User);
        assert!(msg.content.is_text());
    }
}
