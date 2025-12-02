//! Core protocol types and utilities
//!
//! This module contains the fundamental types used throughout the MCP protocol
//! implementation. These types are shared across multiple protocol features
//! and provide the foundational building blocks for the protocol.
//!
//! # Core Types
//!
//! - [`ProtocolVersion`] - Protocol version identifier
//! - [`RequestId`] - JSON-RPC request identifier
//! - [`BaseMetadata`] - Common name/title structure
//! - [`Implementation`] - Implementation information
//! - [`Annotations`] - Common annotation structure
//! - [`Role`] - Message role enum (User/Assistant)
//! - [`JsonRpcError`] - JSON-RPC error structure
//! - [`Timestamp`] - UTC timestamp wrapper

use crate::MessageId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt};

/// Timestamp wrapper for consistent time handling
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Timestamp(pub DateTime<Utc>);

impl Timestamp {
    /// Create a new timestamp with current time
    #[must_use]
    pub fn now() -> Self {
        Self(Utc::now())
    }

    /// Create a timestamp from a DateTime
    #[must_use]
    pub const fn from_datetime(dt: DateTime<Utc>) -> Self {
        Self(dt)
    }

    /// Get the inner DateTime
    #[must_use]
    pub const fn datetime(&self) -> DateTime<Utc> {
        self.0
    }

    /// Get duration since this timestamp
    #[must_use]
    pub fn elapsed(&self) -> chrono::Duration {
        Utc::now() - self.0
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.to_rfc3339())
    }
}

impl From<DateTime<Utc>> for Timestamp {
    fn from(dt: DateTime<Utc>) -> Self {
        Self(dt)
    }
}

/// Protocol version string
pub type ProtocolVersion = String;

/// JSON-RPC request identifier
pub type RequestId = MessageId;

/// URI string (legacy type alias)
///
/// **Note**: For new code, consider using the validated [`crate::types::domain::Uri`] type
/// which provides compile-time type safety and runtime validation.
/// This type alias is kept for backward compatibility.
pub type Uri = String;

/// MIME type (legacy type alias)
///
/// **Note**: For new code, consider using the validated [`crate::types::domain::MimeType`] type
/// which provides compile-time type safety and runtime validation.
/// This type alias is kept for backward compatibility.
pub type MimeType = String;

/// Base64 encoded data (legacy type alias)
///
/// **Note**: For new code, consider using the validated [`crate::types::domain::Base64String`] type
/// which provides compile-time type safety and runtime validation.
/// This type alias is kept for backward compatibility.
pub type Base64String = String;

/// Cursor for pagination
pub type Cursor = String;

/// Standard JSON-RPC error codes per specification
pub mod error_codes {
    /// Parse error - Invalid JSON was received by the server
    pub const PARSE_ERROR: i32 = -32700;
    /// Invalid Request - The JSON sent is not a valid Request object
    pub const INVALID_REQUEST: i32 = -32600;
    /// Method not found - The method does not exist / is not available
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// Invalid params - Invalid method parameter(s)
    pub const INVALID_PARAMS: i32 = -32602;
    /// Internal error - Internal JSON-RPC error
    pub const INTERNAL_ERROR: i32 = -32603;
}

/// JSON-RPC error structure per MCP 2025-06-18 specification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonRpcError {
    /// The error type that occurred
    pub code: i32,
    /// A short description of the error (should be limited to a concise single sentence)
    pub message: String,
    /// Additional information about the error (detailed error information, nested errors, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcError {
    /// Create a new JSON-RPC error
    pub fn new(code: i32, message: String) -> Self {
        Self {
            code,
            message,
            data: None,
        }
    }

    /// Create a new JSON-RPC error with additional data
    pub fn with_data(code: i32, message: String, data: serde_json::Value) -> Self {
        Self {
            code,
            message,
            data: Some(data),
        }
    }

    /// Create a parse error
    pub fn parse_error() -> Self {
        Self::new(error_codes::PARSE_ERROR, "Parse error".to_string())
    }

    /// Create an invalid request error
    pub fn invalid_request() -> Self {
        Self::new(error_codes::INVALID_REQUEST, "Invalid Request".to_string())
    }

    /// Create a method not found error
    pub fn method_not_found(method: &str) -> Self {
        Self::new(
            error_codes::METHOD_NOT_FOUND,
            format!("Method not found: {method}"),
        )
    }

    /// Create an invalid params error
    pub fn invalid_params(details: &str) -> Self {
        Self::new(
            error_codes::INVALID_PARAMS,
            format!("Invalid params: {details}"),
        )
    }

    /// Create an internal error
    pub fn internal_error(details: &str) -> Self {
        Self::new(
            error_codes::INTERNAL_ERROR,
            format!("Internal error: {details}"),
        )
    }
}

/// Base interface for metadata with name (identifier) and title (display name) properties.
/// Per MCP specification 2025-06-18, this is the foundation for Tool, Resource, and Prompt metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseMetadata {
    /// Intended for programmatic or logical use, but used as a display name in past specs or fallback (if title isn't present).
    pub name: String,

    /// Intended for UI and end-user contexts — optimized to be human-readable and easily understood,
    /// even by those unfamiliar with domain-specific terminology.
    ///
    /// If not provided, the name should be used for display (except for Tool,
    /// where `annotations.title` should be given precedence over using `name`, if present).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Implementation information for MCP clients and servers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Implementation {
    /// Implementation name
    pub name: String,
    /// Implementation display title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Implementation version
    pub version: String,
    /// Optional human-readable description of what this implementation does
    ///
    /// This can be used by clients or servers to provide context about their purpose
    /// and capabilities. For example, a server might describe the types of resources
    /// or tools it provides, while a client might describe its intended use case.
    ///
    /// **MCP 2025-11-25 draft**: New field added for better context during initialization
    #[cfg(feature = "mcp-draft")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional set of sized icons that the client can display in a user interface
    ///
    /// Clients that support rendering icons MUST support at least the following MIME types:
    /// - `image/png` - PNG images (safe, universal compatibility)
    /// - `image/jpeg` (and `image/jpg`) - JPEG images (safe, universal compatibility)
    ///
    /// Clients that support rendering icons SHOULD also support:
    /// - `image/svg+xml` - SVG images (scalable but requires security precautions)
    /// - `image/webp` - WebP images (modern, efficient format)
    ///
    /// **MCP 2025-11-25 draft**: New field added (SEP-973)
    #[cfg(feature = "mcp-icons")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icons: Option<Vec<Icon>>,
}

impl Default for Implementation {
    fn default() -> Self {
        Self {
            name: "unknown".to_string(),
            title: None,
            version: "0.0.0".to_string(),
            #[cfg(feature = "mcp-draft")]
            description: None,
            #[cfg(feature = "mcp-icons")]
            icons: None,
        }
    }
}

/// Optional metadata hints that can be attached to MCP objects.
///
/// **Important**: Per the MCP specification, annotations are **weak hints only**.
/// Clients MAY ignore these entirely. They should never be used for security
/// decisions or to make assumptions about actual behavior.
///
/// # Standard Fields
///
/// - **`audience`**: Role-based filtering hint. Values should be `"user"` or `"assistant"`
///   (corresponding to [`Role`]). Clients can use this to filter content presentation.
///
/// - **`priority`**: Subjective importance hint (numeric). Clients often ignore this.
///   No standard range is defined by the MCP spec.
///
/// - **`lastModified`**: ISO 8601 timestamp (e.g., `"2025-11-06T10:30:00Z"`).
///   The most reliably useful field - indicates freshness for caching.
///
/// - **`custom`**: Application-specific extensions. Preserved but rarely interpreted.
///
/// # Usage Notes
///
/// Annotations are optional on:
/// - Content blocks (text, image, audio, resource links)
/// - Resources
/// - Prompts
///
/// For tools, use [`ToolAnnotations`] which includes additional hints like
/// `destructive_hint`, `read_only_hint`, etc. However, the MCP spec warns:
/// *"Clients should never make tool use decisions based on ToolAnnotations
/// received from untrusted servers."*
///
/// # Example
///
/// ```rust
/// use turbomcp_protocol::types::Annotations;
///
/// // Minimal usage (most common)
/// let annotations = Annotations {
///     last_modified: Some("2025-11-06T10:00:00Z".to_string()),
///     ..Default::default()
/// };
///
/// // With audience filtering
/// let annotations = Annotations {
///     audience: Some(vec!["user".to_string()]),
///     last_modified: Some("2025-11-06T10:00:00Z".to_string()),
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Annotations {
    /// Role-based audience hint. Per MCP spec, values should be "user" or "assistant".
    ///
    /// **Note**: This is a weak hint. Clients may ignore it entirely.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<Vec<String>>,
    /// Subjective priority hint (numeric). No standard range defined.
    ///
    /// **Note**: This is a weak hint. Clients often ignore this field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<f64>,
    /// ISO 8601 timestamp of last modification (e.g., "2025-11-06T10:30:00Z").
    ///
    /// Most reliably useful field for cache invalidation and freshness display.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "lastModified")]
    pub last_modified: Option<String>,
    /// Application-specific extensions. Preserved by clients but rarely interpreted.
    #[serde(flatten)]
    pub custom: HashMap<String, serde_json::Value>,
}

/// Role in conversation
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// User role
    User,
    /// Assistant role
    Assistant,
}

/// Base result type for MCP protocol responses
///
/// Per MCP 2025-06-18 specification, all result types should support
/// optional metadata in the `_meta` field.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Result {
    /// Optional metadata per MCP 2025-06-18 specification
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

impl Result {
    /// Create a new result with no metadata
    pub fn new() -> Self {
        Self { _meta: None }
    }

    /// Create a result with metadata
    pub fn with_meta(meta: serde_json::Value) -> Self {
        Self { _meta: Some(meta) }
    }

    /// Add metadata to this result
    pub fn set_meta(&mut self, meta: serde_json::Value) {
        self._meta = Some(meta);
    }
}

impl Default for Result {
    fn default() -> Self {
        Self::new()
    }
}

/// A response that indicates success but carries no data
///
/// Per MCP 2025-06-18 specification, this is simply a Result with no additional fields.
/// This is used for operations where the success of the operation itself
/// is the only meaningful response, such as ping responses.
pub type EmptyResult = Result;

/// Hints to use for model selection
///
/// Keys not declared here are currently left unspecified by the spec and are up
/// to the client to decide how to interpret.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelHint {
    /// Optional model name hint
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Theme specifier for icons (MCP 2025-11-25 draft, SEP-973)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
#[cfg(feature = "mcp-icons")]
pub enum IconTheme {
    /// Icon designed for light backgrounds
    Light,
    /// Icon designed for dark backgrounds
    Dark,
}

/// Icon metadata for visual representation (MCP 2025-11-25 draft, SEP-973)
///
/// Enables servers to expose icons as additional metadata for tools, resources,
/// resource templates, prompts, and implementation information.
///
/// ## MIME Type Support
///
/// Clients MUST support at least:
/// - `image/png` - PNG images (safe, universal compatibility)
/// - `image/jpeg` / `image/jpg` - JPEG images (safe, universal compatibility)
///
/// Clients SHOULD support:
/// - `image/svg+xml` - SVG images (scalable but requires security precautions)
/// - `image/webp` - WebP images (modern, efficient format)
///
/// ## Security Considerations
///
/// - Consumers SHOULD ensure URLs are from the same domain or trusted domains
/// - SVGs can contain executable JavaScript - take appropriate precautions
/// - Data URIs avoid external dependencies but increase message size
///
/// ## Example
///
/// ```json
/// {
///   "src": "https://example.com/weather-icon.png",
///   "mimeType": "image/png",
///   "sizes": ["48x48", "96x96"],
///   "theme": "light"
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg(feature = "mcp-icons")]
pub struct Icon {
    /// A standard URI pointing to an icon resource
    ///
    /// May be an HTTP/HTTPS URL or a `data:` URI with Base64-encoded image data.
    ///
    /// Consumers SHOULD ensure URLs serving icons are from the same domain as
    /// the client/server or a trusted domain.
    ///
    /// Consumers SHOULD take appropriate precautions when consuming SVGs as
    /// they can contain executable JavaScript.
    #[serde(with = "url_string_serde")]
    pub src: url::Url,

    /// Optional MIME type override if the source MIME type is missing or generic
    ///
    /// Examples: `"image/png"`, `"image/jpeg"`, or `"image/svg+xml"`
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,

    /// Optional array of strings specifying sizes at which the icon can be used
    ///
    /// Each string should be in WxH format (e.g., `"48x48"`, `"96x96"`)
    /// or `"any"` for scalable formats like SVG.
    ///
    /// If not provided, the client should assume the icon can be used at any size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sizes: Option<Vec<String>>,

    /// Optional theme specifier
    ///
    /// - `light`: Icon designed for light backgrounds
    /// - `dark`: Icon designed for dark backgrounds
    ///
    /// If not provided, the client should assume the icon can be used with any theme.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<IconTheme>,
}

#[cfg(feature = "mcp-icons")]
mod url_string_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use url::Url;

    pub(super) fn serialize<S>(url: &Url, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(url.as_str())
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> std::result::Result<Url, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Url::parse(&s).map_err(serde::de::Error::custom)
    }
}
