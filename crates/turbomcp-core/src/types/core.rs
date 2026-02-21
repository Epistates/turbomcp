//! Core protocol types shared across MCP.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::jsonrpc::RequestId;

/// Protocol version string
pub type ProtocolVersion = String;

/// Message ID (same as RequestId)
pub type MessageId = RequestId;

/// URI string type
pub type Uri = String;

/// MIME type string
pub type MimeType = String;

/// Base64 encoded string
pub type Base64String = String;

/// Pagination cursor
pub type Cursor = String;

/// Role in conversation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// User role
    #[default]
    User,
    /// Assistant role
    Assistant,
}

/// Icon representation for MCP entities (MCP 2025-11-25)
///
/// Icons can be specified as either:
/// - A data URI (e.g., `data:image/svg+xml;base64,...`)
/// - An HTTPS URL pointing to an image resource
///
/// # Example
///
/// ```rust
/// use turbomcp_core::types::core::Icon;
///
/// // Data URI icon
/// let icon = Icon::data_uri("data:image/svg+xml;base64,PHN2Zz4...");
///
/// // URL icon
/// let icon = Icon::url("https://example.com/icon.svg");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Icon {
    /// Data URI containing embedded image data
    DataUri(String),
    /// HTTPS URL pointing to an image resource
    Url(String),
}

impl Icon {
    /// Create an icon from a data URI
    #[must_use]
    pub fn data_uri(uri: impl Into<String>) -> Self {
        Self::DataUri(uri.into())
    }

    /// Create an icon from a URL
    #[must_use]
    pub fn url(url: impl Into<String>) -> Self {
        Self::Url(url.into())
    }

    /// Get the icon value as a string
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::DataUri(s) | Self::Url(s) => s,
        }
    }

    /// Check if this is a data URI
    #[must_use]
    pub fn is_data_uri(&self) -> bool {
        matches!(self, Self::DataUri(_))
    }

    /// Check if this is a URL
    #[must_use]
    pub fn is_url(&self) -> bool {
        matches!(self, Self::Url(_))
    }
}

impl From<String> for Icon {
    fn from(s: String) -> Self {
        if s.starts_with("data:") {
            Self::DataUri(s)
        } else {
            Self::Url(s)
        }
    }
}

impl From<&str> for Icon {
    fn from(s: &str) -> Self {
        Self::from(s.to_string())
    }
}

/// Implementation information for MCP clients and servers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Implementation {
    /// Implementation name (programmatic identifier)
    pub name: String,
    /// Display title for UI
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Human-readable description (MCP 2025-11-25)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Implementation version
    pub version: String,
    /// Optional icon for the implementation (MCP 2025-11-25)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
}

impl Default for Implementation {
    fn default() -> Self {
        Self {
            name: "unknown".into(),
            title: None,
            description: None,
            version: "0.0.0".into(),
            icon: None,
        }
    }
}

impl Implementation {
    /// Create a new implementation info
    #[must_use]
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            title: None,
            description: None,
            version: version.into(),
            icon: None,
        }
    }

    /// Set the display title
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the description (MCP 2025-11-25)
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the icon (MCP 2025-11-25)
    #[must_use]
    pub fn with_icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
}

/// Base metadata with name and title
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaseMetadata {
    /// Programmatic name/identifier
    pub name: String,
    /// Human-readable display title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl BaseMetadata {
    /// Create metadata with a name and no title.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            title: None,
        }
    }

    /// Set the display title.
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

/// Optional metadata hints for MCP objects.
///
/// Per MCP spec, annotations are **weak hints only** - clients may ignore them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Annotations {
    /// Role-based audience hint ("user" or "assistant")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<Vec<String>>,
    /// Subjective priority hint (no standard range)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<f64>,
    /// ISO 8601 timestamp of last modification
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "lastModified")]
    pub last_modified: Option<String>,
    /// Application-specific extensions
    #[serde(flatten)]
    pub custom: HashMap<String, Value>,
}

/// Base result type for MCP responses
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Result {
    /// Optional metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
}

impl Result {
    /// Create a new result
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with metadata
    #[must_use]
    pub fn with_meta(meta: Value) -> Self {
        Self { _meta: Some(meta) }
    }
}

/// Empty result type
pub type EmptyResult = Result;

/// Model hints for sampling
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelHint {
    /// Optional model name hint
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Model preferences for sampling
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelPreferences {
    /// Model hints in order of preference
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hints: Option<Vec<ModelHint>>,
    /// Cost priority (0-1, lower = prefer cheaper)
    #[serde(rename = "costPriority", skip_serializing_if = "Option::is_none")]
    pub cost_priority: Option<f64>,
    /// Speed priority (0-1, lower = prefer faster)
    #[serde(rename = "speedPriority", skip_serializing_if = "Option::is_none")]
    pub speed_priority: Option<f64>,
    /// Intelligence priority (0-1, lower = prefer smarter)
    #[serde(
        rename = "intelligencePriority",
        skip_serializing_if = "Option::is_none"
    )]
    pub intelligence_priority: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_implementation() {
        let impl_info = Implementation::new("test", "1.0.0")
            .with_title("Test Server")
            .with_description("A test server implementation");
        assert_eq!(impl_info.name, "test");
        assert_eq!(impl_info.title, Some("Test Server".into()));
        assert_eq!(
            impl_info.description,
            Some("A test server implementation".into())
        );
    }

    #[test]
    fn test_implementation_with_icon() {
        let impl_info = Implementation::new("test", "1.0.0")
            .with_icon(Icon::url("https://example.com/icon.svg"));
        assert!(impl_info.icon.is_some());
        assert!(impl_info.icon.as_ref().unwrap().is_url());
    }

    #[test]
    fn test_role_serde() {
        let user = Role::User;
        let json = serde_json::to_string(&user).unwrap();
        assert_eq!(json, "\"user\"");
    }

    #[test]
    fn test_icon_data_uri() {
        let icon = Icon::data_uri("data:image/svg+xml;base64,PHN2Zz4=");
        assert!(icon.is_data_uri());
        assert!(!icon.is_url());
        assert_eq!(icon.as_str(), "data:image/svg+xml;base64,PHN2Zz4=");
    }

    #[test]
    fn test_icon_url() {
        let icon = Icon::url("https://example.com/icon.png");
        assert!(icon.is_url());
        assert!(!icon.is_data_uri());
        assert_eq!(icon.as_str(), "https://example.com/icon.png");
    }

    #[test]
    fn test_icon_from_string() {
        // Data URI detection
        let icon: Icon = "data:image/png;base64,abc".into();
        assert!(icon.is_data_uri());

        // URL detection
        let icon: Icon = "https://example.com/icon.svg".into();
        assert!(icon.is_url());
    }

    #[test]
    fn test_icon_serde() {
        let icon = Icon::url("https://example.com/icon.svg");
        let json = serde_json::to_string(&icon).unwrap();
        assert_eq!(json, "\"https://example.com/icon.svg\"");

        let parsed: Icon = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.as_str(), "https://example.com/icon.svg");
    }
}
