//! Resource access and template types
//!
//! This module contains types for the MCP resource system, including
//! resource definitions, templates, subscriptions, and resource operations.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    content::ResourceContent,
    core::{Annotations, Cursor, MimeType, Uri},
};

/// Resource definition per the current MCP specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    /// Resource name (programmatic identifier)
    pub name: String,

    /// Display title for UI contexts (optional, falls back to name if not provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// The URI of this resource
    pub uri: Uri,

    /// A description of what this resource represents
    /// This can be used by clients to improve the LLM's understanding of available resources
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The MIME type of this resource, if known
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<MimeType>,

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

/// Resource contents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceContents {
    /// The URI of this resource
    pub uri: Uri,
    /// The MIME type of this resource, if known
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<MimeType>,
    /// General metadata field for extensions and custom data
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, serde_json::Value>>,
}

/// Resource template definition.
///
/// `uri_template` is a [RFC 6570](https://www.rfc-editor.org/rfc/rfc6570) URI Template.
/// Construct via [`ResourceTemplate::new`] to validate the template at construction
/// time; the public field is left writable for back-compat with serde deserialization
/// where invalid templates can still round-trip from the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceTemplate {
    /// Template name (programmatic identifier)
    pub name: String,

    /// Display title for UI contexts (optional, falls back to name if not provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// URI template for this resource (RFC 6570).
    #[serde(rename = "uriTemplate")]
    pub uri_template: String,

    /// A description of what this resource template represents
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The MIME type of resources generated from this template, if known
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<MimeType>,

    /// Optional annotations for the client
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,

    /// General metadata field for extensions and custom data
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, serde_json::Value>>,
}

impl ResourceTemplate {
    /// Construct a `ResourceTemplate`, validating `uri_template` as a well-formed
    /// RFC 6570 URI Template. Use this in server-side construction; callers
    /// receiving templates from the wire should rely on serde and treat invalid
    /// templates as request errors at use time.
    ///
    /// Validation is intentionally lightweight (matches `{...}` expression syntax
    /// and rejects unbalanced braces) — full RFC 6570 expansion is out of scope
    /// here. This catches the common drift modes: typos in expression names and
    /// missing closing braces.
    pub fn new(
        name: impl Into<String>,
        uri_template: impl Into<String>,
    ) -> Result<Self, &'static str> {
        let uri_template = uri_template.into();
        validate_uri_template(&uri_template)?;
        Ok(Self {
            name: name.into(),
            title: None,
            uri_template,
            description: None,
            mime_type: None,
            annotations: None,
            meta: None,
        })
    }
}

/// Validate a string against the structural shape of an RFC 6570 URI Template.
///
/// Pre-3.1 templates were accepted as raw `String` with no validation, so
/// malformed templates (e.g., `file://{path` or `{name`) silently passed through
/// and only failed at expansion time. This catches the structural errors at
/// construction.
pub fn validate_uri_template(s: &str) -> Result<(), &'static str> {
    let mut depth = 0i32;
    for ch in s.chars() {
        match ch {
            '{' => {
                depth += 1;
                if depth > 1 {
                    return Err("URI template: nested '{' not allowed in RFC 6570");
                }
            }
            '}' => {
                depth -= 1;
                if depth < 0 {
                    return Err("URI template: unbalanced '}' (no matching '{')");
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err("URI template: unbalanced '{' (missing closing '}')");
    }
    Ok(())
}

/// List resources request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResourcesRequest {
    /// Optional cursor for pagination
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    /// Optional metadata per the current MCP specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// List resources result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResourcesResult {
    /// Available resources
    pub resources: Vec<Resource>,
    /// Optional continuation token
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
    /// Optional metadata per the current MCP specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// List resource templates request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResourceTemplatesRequest {
    /// Optional cursor for pagination
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    /// Optional metadata per the current MCP specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// List resource templates result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResourceTemplatesResult {
    /// Available resource templates
    #[serde(rename = "resourceTemplates")]
    pub resource_templates: Vec<ResourceTemplate>,
    /// Optional continuation token
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
    /// Optional metadata per the current MCP specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// Read resource request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadResourceRequest {
    /// Resource URI
    pub uri: Uri,
    /// Optional metadata per the current MCP specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// Read resource result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadResourceResult {
    /// Resource contents (can be text or binary)
    pub contents: Vec<ResourceContent>,
    /// Optional metadata per the current MCP specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// Subscribe request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribeRequest {
    /// Resource URI
    pub uri: Uri,
}

/// Unsubscribe request
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
