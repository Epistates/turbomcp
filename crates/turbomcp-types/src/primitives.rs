//! Primitive wire-format types shared across the MCP protocol.
//!
//! These are the domain newtypes and forward-compatible version enum used
//! throughout MCP messages. They are `no_std + alloc`-compatible.
//!
//! ## Inventory
//!
//! - [`ProtocolVersion`] — MCP spec version enum with forward-compatible
//!   [`Unknown`](ProtocolVersion::Unknown) variant for unrecognised strings.
//! - [`Uri`] — transparent `String` newtype for URIs.
//! - [`MimeType`] — transparent `String` newtype for MIME types.
//! - [`Base64String`] — transparent `String` newtype for base64-encoded data.
//! - [`BaseMetadata`] — `{ name, title }` used as a base for many MCP entities.
//! - [`Cursor`] — pagination cursor alias (`String`).

use core::fmt;
use core::ops::Deref;

#[cfg(not(feature = "std"))]
use alloc::string::String;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

// =============================================================================
// ProtocolVersion
// =============================================================================

/// MCP protocol version.
///
/// Represents a known or unknown MCP specification version. Known versions get
/// first-class enum variants; unknown version strings are preserved via
/// [`Unknown`](ProtocolVersion::Unknown) for forward compatibility (e.g. proxies
/// and protocol analyzers that handle arbitrary versions).
///
/// ## Ordering
///
/// Known versions are ordered by specification release date.
/// [`Unknown`](ProtocolVersion::Unknown) sorts after all known versions.
///
/// ## Serialization
///
/// Serializes to/from the canonical version string (e.g. `"2025-11-25"`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum ProtocolVersion {
    /// MCP specification 2025-06-18.
    V2025_06_18,
    /// MCP specification 2025-11-25 (current stable).
    #[default]
    V2025_11_25,
    /// Draft specification (`DRAFT-2026-v1`).
    Draft,
    /// Unknown / future protocol version (preserved for forward compatibility).
    Unknown(String),
}

impl ProtocolVersion {
    /// The latest stable protocol version.
    pub const LATEST: Self = Self::V2025_11_25;

    /// All stable (released) protocol versions, oldest to newest.
    ///
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

    /// Whether this is a named (non-`Unknown`) protocol version.
    #[must_use]
    pub fn is_known(&self) -> bool {
        !matches!(self, Self::Unknown(_))
    }

    /// Whether this is a stable (released) protocol version.
    ///
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
            (Self::Unknown(a), Self::Unknown(b)) => a.cmp(b),
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

// =============================================================================
// Uri
// =============================================================================

/// Transparent `String` newtype for URIs.
///
/// No validation is performed on construction. If callers need to verify the
/// URI parses cleanly, they can apply their own check (for example
/// `url::Url::parse(uri.as_str())` from the `url` crate).
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

// =============================================================================
// MimeType
// =============================================================================

/// Transparent `String` newtype for MIME types.
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

// =============================================================================
// Base64String
// =============================================================================

/// Transparent `String` newtype for base64-encoded payloads.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Base64String(String);

impl Base64String {
    /// Create a base64 wrapper without additional validation.
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

// =============================================================================
// BaseMetadata
// =============================================================================

/// Base metadata shared by MCP entities that carry a programmatic name and an
/// optional human-readable title.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaseMetadata {
    /// Programmatic name / identifier.
    pub name: String,
    /// Human-readable display title.
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

// =============================================================================
// Cursor
// =============================================================================

/// Pagination cursor (opaque string per MCP 2025-11-25).
pub type Cursor = String;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_version_serde_roundtrip() {
        let v = ProtocolVersion::V2025_11_25;
        let s = serde_json::to_string(&v).unwrap();
        assert_eq!(s, "\"2025-11-25\"");
        let back: ProtocolVersion = serde_json::from_str(&s).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn protocol_version_unknown_preserves_string() {
        let v: ProtocolVersion = "future-version".into();
        assert_eq!(v, ProtocolVersion::Unknown("future-version".into()));
        assert!(!v.is_known());
        assert!(!v.is_stable());
    }

    #[test]
    fn protocol_version_ordering() {
        assert!(ProtocolVersion::V2025_06_18 < ProtocolVersion::V2025_11_25);
        assert!(ProtocolVersion::V2025_11_25 < ProtocolVersion::Draft);
        assert!(ProtocolVersion::Draft < ProtocolVersion::Unknown("x".into()));
    }

    #[test]
    fn uri_transparent_serde() {
        let uri = Uri::new("file:///path/to/file.txt");
        let s = serde_json::to_string(&uri).unwrap();
        assert_eq!(s, "\"file:///path/to/file.txt\"");
        let back: Uri = serde_json::from_str(&s).unwrap();
        assert_eq!(back, uri);
    }

    #[test]
    fn mime_type_and_base64_roundtrips() {
        let m = MimeType::new("application/json");
        assert_eq!(m.as_str(), "application/json");
        let b = Base64String::new("aGVsbG8=");
        assert_eq!(b.as_str(), "aGVsbG8=");
    }

    #[test]
    fn base_metadata_builder() {
        let meta = BaseMetadata::new("my_tool").with_title("My Tool");
        assert_eq!(meta.name, "my_tool");
        assert_eq!(meta.title.as_deref(), Some("My Tool"));
        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains("\"name\":\"my_tool\""));
        assert!(json.contains("\"title\":\"My Tool\""));
    }

    #[test]
    fn base_metadata_omits_absent_title() {
        let meta = BaseMetadata::new("x");
        let json = serde_json::to_string(&meta).unwrap();
        assert!(!json.contains("title"));
    }
}
