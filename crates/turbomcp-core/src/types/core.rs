//! Core protocol types shared across MCP.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use core::ops::Deref;
use hashbrown::HashMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::jsonrpc::RequestId;

/// MCP protocol version.
///
/// Represents a known or unknown MCP specification version. Known versions get
/// first-class enum variants; unknown version strings are preserved via
/// [`Unknown`](ProtocolVersion::Unknown) for forward compatibility (e.g. proxies
/// and protocol analyzers that handle arbitrary versions).
///
/// # Ordering
///
/// Known versions are ordered by specification release date. [`Unknown`](ProtocolVersion::Unknown)
/// sorts after all known versions.
///
/// # Serialization
///
/// Serializes to/from the canonical version string (e.g. `"2025-11-25"`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum ProtocolVersion {
    /// MCP specification 2025-06-18
    V2025_06_18,
    /// MCP specification 2025-11-25 (current stable)
    #[default]
    V2025_11_25,
    /// Draft specification (DRAFT-2026-v1)
    Draft,
    /// Unknown/future protocol version (preserved for forward compatibility)
    Unknown(String),
}

impl ProtocolVersion {
    /// The latest stable protocol version.
    pub const LATEST: Self = Self::V2025_11_25;

    /// All stable (released) protocol versions, oldest to newest.
    /// Does not include [`Draft`](Self::Draft) or [`Unknown`](Self::Unknown).
    pub const STABLE: &[Self] = &[Self::V2025_06_18, Self::V2025_11_25];

    /// The canonical version string for this protocol version.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::V2025_06_18 => "2025-06-18",
            Self::V2025_11_25 => "2025-11-25",
            Self::Draft => "DRAFT-2026-v1",
            Self::Unknown(s) => s.as_str(),
        }
    }

    /// Whether this is a named (non-Unknown) protocol version.
    /// Returns `true` for all variants except [`Unknown`](Self::Unknown).
    #[must_use]
    pub fn is_known(&self) -> bool {
        !matches!(self, Self::Unknown(_))
    }

    /// Whether this is a stable (released) protocol version.
    /// Returns `false` for [`Draft`](Self::Draft) and [`Unknown`](Self::Unknown).
    #[must_use]
    pub fn is_stable(&self) -> bool {
        matches!(self, Self::V2025_06_18 | Self::V2025_11_25)
    }

    /// Whether this is the draft specification.
    #[must_use]
    pub fn is_draft(&self) -> bool {
        matches!(self, Self::Draft)
    }

    /// Ordinal for comparison (known versions ordered by release date).
    fn ordinal(&self) -> u32 {
        match self {
            Self::V2025_06_18 => 1,
            Self::V2025_11_25 => 2,
            Self::Draft => 3,
            Self::Unknown(_) => u32::MAX,
        }
    }
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialOrd for ProtocolVersion {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ProtocolVersion {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        match (self, other) {
            // Two Unknown variants: compare by inner string for consistency with PartialEq
            (Self::Unknown(a), Self::Unknown(b)) => a.cmp(b),
            // Otherwise compare by ordinal (release date order)
            _ => self.ordinal().cmp(&other.ordinal()),
        }
    }
}

impl From<&str> for ProtocolVersion {
    fn from(s: &str) -> Self {
        match s {
            "2025-06-18" => Self::V2025_06_18,
            "2025-11-25" => Self::V2025_11_25,
            "DRAFT-2026-v1" => Self::Draft,
            other => Self::Unknown(other.into()),
        }
    }
}

impl From<String> for ProtocolVersion {
    fn from(s: String) -> Self {
        match s.as_str() {
            "2025-06-18" => Self::V2025_06_18,
            "2025-11-25" => Self::V2025_11_25,
            "DRAFT-2026-v1" => Self::Draft,
            _ => Self::Unknown(s),
        }
    }
}

impl From<ProtocolVersion> for String {
    fn from(v: ProtocolVersion) -> Self {
        match v {
            ProtocolVersion::Unknown(s) => s,
            other => other.as_str().into(),
        }
    }
}

impl Serialize for ProtocolVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> core::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ProtocolVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> core::result::Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(ProtocolVersion::from(s))
    }
}

impl PartialEq<&str> for ProtocolVersion {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<ProtocolVersion> for &str {
    fn eq(&self, other: &ProtocolVersion) -> bool {
        *self == other.as_str()
    }
}

/// Message ID (same as RequestId)
pub type MessageId = RequestId;

/// URI string type.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Uri(String);

impl Uri {
    /// Create a URI wrapper without additional validation.
    #[must_use]
    pub fn new(uri: impl Into<String>) -> Self {
        Self(uri.into())
    }

    /// Borrow the inner string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume into the underlying string.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for Uri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Deref for Uri {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for Uri {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for Uri {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Uri {
    fn from(value: &str) -> Self {
        Self(value.into())
    }
}

impl From<Uri> for String {
    fn from(value: Uri) -> Self {
        value.0
    }
}

impl PartialEq<&str> for Uri {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<Uri> for &str {
    fn eq(&self, other: &Uri) -> bool {
        *self == other.as_str()
    }
}

/// MIME type string.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MimeType(String);

impl MimeType {
    /// Create a MIME type wrapper without additional validation.
    #[must_use]
    pub fn new(mime_type: impl Into<String>) -> Self {
        Self(mime_type.into())
    }

    /// Borrow the inner string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MimeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Deref for MimeType {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for MimeType {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for MimeType {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for MimeType {
    fn from(value: &str) -> Self {
        Self(value.into())
    }
}

impl From<MimeType> for String {
    fn from(value: MimeType) -> Self {
        value.0
    }
}

impl PartialEq<&str> for MimeType {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<MimeType> for &str {
    fn eq(&self, other: &MimeType) -> bool {
        *self == other.as_str()
    }
}

/// Base64 encoded string.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Base64String(String);

impl Base64String {
    /// Create a Base64 wrapper without additional validation.
    #[must_use]
    pub fn new(data: impl Into<String>) -> Self {
        Self(data.into())
    }

    /// Borrow the inner string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Base64String {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Deref for Base64String {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for Base64String {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for Base64String {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Base64String {
    fn from(value: &str) -> Self {
        Self(value.into())
    }
}

impl From<Base64String> for String {
    fn from(value: Base64String) -> Self {
        value.0
    }
}

impl PartialEq<&str> for Base64String {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<Base64String> for &str {
    fn eq(&self, other: &Base64String) -> bool {
        *self == other.as_str()
    }
}

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
    /// Optional icons for the implementation (MCP 2025-11-25)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icons: Option<Vec<Icon>>,
    /// Optional website URL for the implementation (MCP 2025-11-25)
    #[serde(rename = "websiteUrl", skip_serializing_if = "Option::is_none")]
    pub website_url: Option<String>,
}

impl Default for Implementation {
    fn default() -> Self {
        Self {
            name: "unknown".into(),
            title: None,
            description: None,
            version: "0.0.0".into(),
            icons: None,
            website_url: None,
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
            icons: None,
            website_url: None,
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
        self.icons.get_or_insert_with(Vec::new).push(icon);
        self
    }

    /// Set the website URL (MCP 2025-11-25)
    #[must_use]
    pub fn with_website_url(mut self, website_url: impl Into<String>) -> Self {
        self.website_url = Some(website_url.into());
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
        assert_eq!(impl_info.icons.as_ref().map(Vec::len), Some(1));
        assert!(impl_info.icons.as_ref().unwrap()[0].is_url());
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
