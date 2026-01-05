//! Core protocol types shared across MCP.

use alloc::string::String;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// User role
    User,
    /// Assistant role
    Assistant,
}

impl Default for Role {
    fn default() -> Self {
        Self::User
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
    /// Implementation version
    pub version: String,
}

impl Default for Implementation {
    fn default() -> Self {
        Self {
            name: "unknown".into(),
            title: None,
            version: "0.0.0".into(),
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
            version: version.into(),
        }
    }

    /// Set the display title
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

/// Base metadata with name and title
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseMetadata {
    /// Programmatic name/identifier
    pub name: String,
    /// Human-readable display title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
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
    #[serde(rename = "intelligencePriority", skip_serializing_if = "Option::is_none")]
    pub intelligence_priority: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_implementation() {
        let impl_info = Implementation::new("test", "1.0.0").with_title("Test Server");
        assert_eq!(impl_info.name, "test");
        assert_eq!(impl_info.title, Some("Test Server".into()));
    }

    #[test]
    fn test_role_serde() {
        let user = Role::User;
        let json = serde_json::to_string(&user).unwrap();
        assert_eq!(json, "\"user\"");
    }
}
