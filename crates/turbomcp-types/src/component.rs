//! Component metadata types for tags and versioning.
//!
//! This module provides types for organizing and filtering MCP components
//! (tools, resources, prompts) using tags and semantic versioning.
//!
//! # Tags
//!
//! Tags allow categorizing components for filtering and access control:
//!
//! ```rust
//! use turbomcp_types::component::{ComponentMeta, ComponentFilter};
//!
//! let meta = ComponentMeta::new()
//!     .with_tags(["admin", "dangerous"])
//!     .with_version("2.0.0");
//!
//! // Filter by tags
//! let filter = ComponentFilter::with_tags(["admin"]);
//! assert!(filter.matches(&meta));
//! ```
//!
//! # Versioning
//!
//! Version strings follow semantic versioning (major.minor.patch):
//!
//! ```rust
//! use turbomcp_types::component::ComponentMeta;
//!
//! let v1 = ComponentMeta::new().with_version("1.0.0");
//! let v2 = ComponentMeta::new().with_version("2.0.0");
//! ```

use alloc::collections::BTreeSet;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(feature = "std")]
use std::collections::HashMap;

#[cfg(not(feature = "std"))]
use alloc::collections::BTreeMap as HashMap;

/// Metadata for an MCP component (tool, resource, or prompt).
///
/// This struct holds tags and version information that can be used for:
/// - Filtering components by category
/// - Access control (e.g., admin-only tools)
/// - API versioning and evolution
/// - Progressive disclosure patterns
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentMeta {
    /// Tags for categorization and filtering.
    ///
    /// Common tag patterns:
    /// - `admin` - Administrative operations
    /// - `readonly` - Read-only operations
    /// - `dangerous` - Operations with side effects
    /// - `deprecated` - Deprecated components
    /// - `experimental` - Experimental features
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,

    /// Semantic version of the component.
    ///
    /// Follows semver format: `major.minor.patch`
    /// Used for API evolution and client compatibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl ComponentMeta {
    /// Create empty component metadata.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            tags: Vec::new(),
            version: None,
        }
    }

    /// Set tags from an iterable.
    #[must_use]
    pub fn with_tags<I, S>(mut self, tags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.tags = tags.into_iter().map(Into::into).collect();
        self
    }

    /// Add a single tag.
    #[must_use]
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Set the version.
    #[must_use]
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Check if the component has a specific tag.
    #[must_use]
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }

    /// Check if the component has any of the specified tags.
    #[must_use]
    pub fn has_any_tag<S: AsRef<str>>(&self, tags: &[S]) -> bool {
        tags.iter().any(|t| self.has_tag(t.as_ref()))
    }

    /// Check if the component has all of the specified tags.
    #[must_use]
    pub fn has_all_tags<S: AsRef<str>>(&self, tags: &[S]) -> bool {
        tags.iter().all(|t| self.has_tag(t.as_ref()))
    }

    /// Check if the component matches a version requirement.
    ///
    /// Simple exact match for now - could be extended for semver ranges.
    #[must_use]
    pub fn matches_version(&self, version: &str) -> bool {
        self.version.as_ref().is_some_and(|v| v == version)
    }

    /// Convert to a JSON Value for the `meta` field.
    #[must_use]
    pub fn to_meta_value(&self) -> Option<HashMap<String, Value>> {
        if self.tags.is_empty() && self.version.is_none() {
            return None;
        }

        let mut map = HashMap::new();

        if !self.tags.is_empty() {
            map.insert(
                "tags".to_string(),
                Value::Array(self.tags.iter().map(|t| Value::String(t.clone())).collect()),
            );
        }

        if let Some(ref version) = self.version {
            map.insert("version".to_string(), Value::String(version.clone()));
        }

        Some(map)
    }

    /// Parse from a meta HashMap (from Tool/Resource/Prompt).
    #[must_use]
    pub fn from_meta_value(meta: Option<&HashMap<String, Value>>) -> Self {
        let Some(meta) = meta else {
            return Self::new();
        };

        let tags = meta
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(ToString::to_string))
                    .collect()
            })
            .unwrap_or_default();

        let version = meta
            .get("version")
            .and_then(|v| v.as_str())
            .map(ToString::to_string);

        Self { tags, version }
    }
}

/// Filter criteria for selecting components.
///
/// Used by the visibility layer to enable/disable components based on tags.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComponentFilter {
    /// Tags that must be present (any match).
    pub include_tags: BTreeSet<String>,

    /// Tags that must NOT be present (any match excludes).
    pub exclude_tags: BTreeSet<String>,

    /// Specific versions to include.
    pub include_versions: BTreeSet<String>,
}

impl ComponentFilter {
    /// Create an empty filter (matches everything).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a filter that matches any of the given tags.
    #[must_use]
    pub fn with_tags<I, S>(tags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            include_tags: tags.into_iter().map(Into::into).collect(),
            ..Default::default()
        }
    }

    /// Create a filter that excludes the given tags.
    #[must_use]
    pub fn excluding_tags<I, S>(tags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            exclude_tags: tags.into_iter().map(Into::into).collect(),
            ..Default::default()
        }
    }

    /// Add tags to include.
    #[must_use]
    pub fn include<I, S>(mut self, tags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.include_tags.extend(tags.into_iter().map(Into::into));
        self
    }

    /// Add tags to exclude.
    #[must_use]
    pub fn exclude<I, S>(mut self, tags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.exclude_tags.extend(tags.into_iter().map(Into::into));
        self
    }

    /// Add versions to include.
    #[must_use]
    pub fn versions<I, S>(mut self, versions: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.include_versions
            .extend(versions.into_iter().map(Into::into));
        self
    }

    /// Check if a component matches this filter.
    ///
    /// Returns `true` if:
    /// - No include_tags specified OR component has any of include_tags
    /// - Component does NOT have any of exclude_tags
    /// - No include_versions specified OR component version is in include_versions
    #[must_use]
    pub fn matches(&self, meta: &ComponentMeta) -> bool {
        // Check exclude tags first (any match = excluded)
        if !self.exclude_tags.is_empty() && meta.tags.iter().any(|t| self.exclude_tags.contains(t))
        {
            return false;
        }

        // Check include tags (empty = match all, otherwise any match)
        if !self.include_tags.is_empty() && !meta.tags.iter().any(|t| self.include_tags.contains(t))
        {
            return false;
        }

        // Check versions (empty = match all)
        if !self.include_versions.is_empty() {
            if let Some(ref version) = meta.version {
                if !self.include_versions.contains(version) {
                    return false;
                }
            } else {
                return false; // No version but versions required
            }
        }

        true
    }
}

/// Unique identifier for a component.
///
/// Combines name and optional version for disambiguating multiple versions
/// of the same component.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ComponentKey {
    /// Component name.
    pub name: String,

    /// Optional version for disambiguation.
    pub version: Option<String>,
}

impl ComponentKey {
    /// Create a key with just a name.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: None,
        }
    }

    /// Create a key with name and version.
    #[must_use]
    pub fn with_version(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: Some(version.into()),
        }
    }

    /// Get the display name (name@version if versioned).
    #[must_use]
    pub fn display_name(&self) -> String {
        match &self.version {
            Some(v) => alloc::format!("{}@{}", self.name, v),
            None => self.name.clone(),
        }
    }
}

/// Macro for ergonomic tag filter creation.
///
/// # Example
///
/// ```rust
/// use turbomcp_types::tags;
///
/// let filter = tags!["admin", "dangerous"];
/// ```
#[macro_export]
macro_rules! tags {
    () => {
        $crate::component::ComponentFilter::new()
    };
    ($($tag:expr),+ $(,)?) => {
        $crate::component::ComponentFilter::with_tags([$($tag),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_component_meta_tags() {
        let meta = ComponentMeta::new()
            .with_tags(["admin", "dangerous"])
            .with_version("2.0.0");

        assert!(meta.has_tag("admin"));
        assert!(meta.has_tag("dangerous"));
        assert!(!meta.has_tag("readonly"));
        assert!(meta.has_any_tag(&["admin", "readonly"]));
        assert!(meta.has_all_tags(&["admin", "dangerous"]));
        assert!(!meta.has_all_tags(&["admin", "readonly"]));
    }

    #[test]
    fn test_component_meta_version() {
        let meta = ComponentMeta::new().with_version("1.2.3");

        assert!(meta.matches_version("1.2.3"));
        assert!(!meta.matches_version("2.0.0"));
    }

    #[test]
    fn test_component_meta_to_value() {
        let meta = ComponentMeta::new()
            .with_tags(["admin"])
            .with_version("1.0.0");

        let value = meta.to_meta_value().unwrap();
        assert!(value.contains_key("tags"));
        assert!(value.contains_key("version"));
    }

    #[test]
    fn test_component_meta_round_trip() {
        let original = ComponentMeta::new()
            .with_tags(["admin", "readonly"])
            .with_version("2.1.0");

        let value = original.to_meta_value();
        let parsed = ComponentMeta::from_meta_value(value.as_ref());

        assert_eq!(original, parsed);
    }

    #[test]
    fn test_filter_matches() {
        let admin_meta = ComponentMeta::new().with_tags(["admin"]);
        let user_meta = ComponentMeta::new().with_tags(["user"]);
        let both_meta = ComponentMeta::new().with_tags(["admin", "user"]);

        // Include filter
        let admin_filter = ComponentFilter::with_tags(["admin"]);
        assert!(admin_filter.matches(&admin_meta));
        assert!(!admin_filter.matches(&user_meta));
        assert!(admin_filter.matches(&both_meta));

        // Exclude filter
        let no_admin = ComponentFilter::excluding_tags(["admin"]);
        assert!(!no_admin.matches(&admin_meta));
        assert!(no_admin.matches(&user_meta));
        assert!(!no_admin.matches(&both_meta));

        // Empty filter matches all
        let all_filter = ComponentFilter::new();
        assert!(all_filter.matches(&admin_meta));
        assert!(all_filter.matches(&user_meta));
    }

    #[test]
    fn test_filter_version() {
        let v1 = ComponentMeta::new().with_version("1.0.0");
        let v2 = ComponentMeta::new().with_version("2.0.0");
        let no_version = ComponentMeta::new();

        let v1_filter = ComponentFilter::new().versions(["1.0.0"]);
        assert!(v1_filter.matches(&v1));
        assert!(!v1_filter.matches(&v2));
        assert!(!v1_filter.matches(&no_version));
    }

    #[test]
    fn test_component_key() {
        let key = ComponentKey::new("my_tool");
        assert_eq!(key.display_name(), "my_tool");

        let versioned = ComponentKey::with_version("my_tool", "2.0");
        assert_eq!(versioned.display_name(), "my_tool@2.0");
    }

    #[test]
    fn test_tags_macro() {
        let empty = tags![];
        assert!(empty.include_tags.is_empty());

        let filter = tags!["admin", "dangerous"];
        assert!(filter.include_tags.contains("admin"));
        assert!(filter.include_tags.contains("dangerous"));
    }
}
