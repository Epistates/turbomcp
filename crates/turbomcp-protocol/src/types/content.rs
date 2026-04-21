//! Message content types.
//!
//! The `ContentBlock` union + leaf content structs are canonically defined in
//! [`turbomcp_types`] as of v3.2. `ContentBlock` is a type alias over
//! [`turbomcp_types::Content`] so the two names refer to the same Rust type.
//!
//! # Content types
//!
//! - [`TextContent`], [`ImageContent`], [`AudioContent`] — inline content
//! - [`ResourceLink`] — reference to an external resource
//! - [`EmbeddedResource`] + [`ResourceContents`] (plural, `Text | Blob` union)
//! - `ResourceContent` — backward-compatibility alias for [`ResourceContents`]
//! - [`ContentType`] — `Json | Binary | Text` (turbomcp-internal; not MCP spec)

use serde::{Deserialize, Serialize};

pub use turbomcp_types::{
    AudioContent, BlobResourceContents, Content, EmbeddedResource, ImageContent, ResourceContents,
    ResourceLink, TextContent, TextResourceContents,
};

/// Backward-compatibility alias — canonical name is [`ResourceContents`].
pub type ResourceContent = ResourceContents;

/// Spec-aligned alias for the `ContentBlock` union per MCP 2025-11-25.
///
/// Canonical Rust name in [`turbomcp_types`] is `Content`; this alias
/// surfaces the spec name. Both refer to the same type.
pub type ContentBlock = Content;

/// Content type enumeration (turbomcp-internal transport discriminator; not part of the MCP spec).
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
