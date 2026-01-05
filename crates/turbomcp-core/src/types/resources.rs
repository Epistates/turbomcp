//! Resource types for MCP.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::content::ResourceContent;
use super::core::{Annotations, MimeType, Uri};

/// Resource definition
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Resource {
    /// Resource URI
    pub uri: Uri,
    /// Human-readable name
    pub name: String,
    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional display title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional MIME type
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<MimeType>,
    /// Size in bytes (if known)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Optional annotations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
}

impl Resource {
    /// Create a new resource
    #[must_use]
    pub fn new(uri: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            name: name.into(),
            ..Default::default()
        }
    }

    /// Set description
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set MIME type
    #[must_use]
    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }
}

/// Resource template for dynamic resources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceTemplate {
    /// URI template (RFC 6570)
    #[serde(rename = "uriTemplate")]
    pub uri_template: String,
    /// Human-readable name
    pub name: String,
    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional display title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional MIME type
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<MimeType>,
    /// Optional annotations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
}

impl ResourceTemplate {
    /// Create a new resource template
    #[must_use]
    pub fn new(uri_template: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            uri_template: uri_template.into(),
            name: name.into(),
            description: None,
            title: None,
            mime_type: None,
            annotations: None,
        }
    }
}

/// Request to list resources
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListResourcesRequest {
    /// Pagination cursor
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Optional metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
}

/// Response with list of resources
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListResourcesResult {
    /// Available resources
    pub resources: Vec<Resource>,
    /// Next page cursor
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// Optional metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
}

/// Request to list resource templates
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListResourceTemplatesRequest {
    /// Pagination cursor
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Optional metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
}

/// Response with list of resource templates
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListResourceTemplatesResult {
    /// Available templates
    #[serde(rename = "resourceTemplates")]
    pub resource_templates: Vec<ResourceTemplate>,
    /// Next page cursor
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// Optional metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
}

/// Request to read a resource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadResourceRequest {
    /// Resource URI to read
    pub uri: Uri,
    /// Optional metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
}

/// Result of reading a resource
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReadResourceResult {
    /// Resource contents
    pub contents: Vec<ResourceContent>,
    /// Optional metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
}

/// Request to subscribe to resource updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribeRequest {
    /// Resource URI to subscribe to
    pub uri: Uri,
    /// Optional metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
}

/// Request to unsubscribe from resource updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsubscribeRequest {
    /// Resource URI to unsubscribe from
    pub uri: Uri,
    /// Optional metadata
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
}

/// Notification that a resource was updated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUpdatedNotification {
    /// Updated resource URI
    pub uri: Uri,
}

/// Notification that the resource list changed
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceListChangedNotification {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_builder() {
        let resource = Resource::new("file:///test.txt", "Test File")
            .with_description("A test file")
            .with_mime_type("text/plain");

        assert_eq!(resource.uri, "file:///test.txt");
        assert_eq!(resource.mime_type, Some("text/plain".into()));
    }
}
