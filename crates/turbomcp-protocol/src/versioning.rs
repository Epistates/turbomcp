//! # Protocol Versioning and Compatibility
//!
//! This module provides comprehensive protocol version management and compatibility
//! checking for MCP implementations.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

/// Version manager for handling protocol versions
#[derive(Debug, Clone)]
pub struct VersionManager {
    /// Supported versions in order of preference
    supported_versions: Vec<Version>,
    /// Current protocol version
    current_version: Version,
}

/// Semantic version representation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    /// Year component
    pub year: u16,
    /// Month component  
    pub month: u8,
    /// Day component
    pub day: u8,
}

/// Version compatibility result
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionCompatibility {
    /// Versions are fully compatible
    Compatible,
    /// Versions are compatible with warnings
    CompatibleWithWarnings(Vec<String>),
    /// Versions are incompatible
    Incompatible(String),
}

/// Version requirement specification
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionRequirement {
    /// Exact version match
    Exact(Version),
    /// Minimum version required
    Minimum(Version),
    /// Maximum version supported
    Maximum(Version),
    /// Version range (inclusive)
    Range(Version, Version),
    /// Any version from the list
    Any(Vec<Version>),
}

impl Version {
    /// Create a new version
    ///
    /// # Errors
    ///
    /// Returns [`VersionError::InvalidMonth`] if month is not in range 1-12.
    /// Returns [`VersionError::InvalidDay`] if day is invalid for the given month.
    pub fn new(year: u16, month: u8, day: u8) -> Result<Self, VersionError> {
        if !(1..=12).contains(&month) {
            return Err(VersionError::InvalidMonth(month.to_string()));
        }

        if !(1..=31).contains(&day) {
            return Err(VersionError::InvalidDay(day.to_string()));
        }

        // Basic month/day validation
        if month == 2 && day > 29 {
            return Err(VersionError::InvalidDay(format!(
                "{} (invalid for February)",
                day
            )));
        }

        if matches!(month, 4 | 6 | 9 | 11) && day > 30 {
            return Err(VersionError::InvalidDay(format!(
                "{} (month {} only has 30 days)",
                day, month
            )));
        }

        Ok(Self { year, month, day })
    }

    /// Get the current MCP protocol version
    pub fn current() -> Self {
        Self {
            year: 2025,
            month: 6,
            day: 18,
        }
    }

    /// Check if this version is newer than another
    pub fn is_newer_than(&self, other: &Version) -> bool {
        self > other
    }

    /// Check if this version is older than another
    pub fn is_older_than(&self, other: &Version) -> bool {
        self < other
    }

    /// Check if this version is compatible with another
    pub fn is_compatible_with(&self, other: &Version) -> bool {
        // For MCP, we consider versions compatible if they're the same
        // or if the difference is minor (same year)
        self.year == other.year
    }

    /// Get version as a date string (YYYY-MM-DD)
    pub fn to_date_string(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// Parse version from date string
    ///
    /// # Errors
    ///
    /// Returns [`VersionError`] if the string is not in `YYYY-MM-DD` format
    /// or contains invalid date components.
    pub fn from_date_string(s: &str) -> Result<Self, VersionError> {
        s.parse()
    }

    /// Get all known MCP versions
    pub fn known_versions() -> Vec<Version> {
        vec![
            Version::new(2025, 6, 18).unwrap(), // Current
            Version::new(2024, 11, 5).unwrap(), // Previous
            Version::new(2024, 6, 25).unwrap(), // Older
        ]
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_date_string())
    }
}

impl FromStr for Version {
    type Err = VersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('-').collect();

        if parts.len() != 3 {
            return Err(VersionError::InvalidFormat(s.to_string()));
        }

        let year = parts[0]
            .parse::<u16>()
            .map_err(|_| VersionError::InvalidYear(parts[0].to_string()))?;

        let month = parts[1]
            .parse::<u8>()
            .map_err(|_| VersionError::InvalidMonth(parts[1].to_string()))?;

        let day = parts[2]
            .parse::<u8>()
            .map_err(|_| VersionError::InvalidDay(parts[2].to_string()))?;

        Self::new(year, month, day)
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.year, self.month, self.day).cmp(&(other.year, other.month, other.day))
    }
}

impl VersionManager {
    /// Create a new version manager
    ///
    /// # Errors
    ///
    /// Returns [`VersionError::NoSupportedVersions`] if the provided vector is empty.
    pub fn new(supported_versions: Vec<Version>) -> Result<Self, VersionError> {
        if supported_versions.is_empty() {
            return Err(VersionError::NoSupportedVersions);
        }

        let mut versions = supported_versions;
        versions.sort_by(|a, b| b.cmp(a)); // Sort newest first

        let current_version = versions[0].clone();

        Ok(Self {
            supported_versions: versions,
            current_version,
        })
    }

    /// Create a version manager with default MCP versions
    pub fn with_default_versions() -> Self {
        Self::new(Version::known_versions()).unwrap()
    }
    /// Get the current version
    pub fn current_version(&self) -> &Version {
        &self.current_version
    }

    /// Get all supported versions
    pub fn supported_versions(&self) -> &[Version] {
        &self.supported_versions
    }

    /// Check if a version is supported
    pub fn is_version_supported(&self, version: &Version) -> bool {
        self.supported_versions.contains(version)
    }

    /// Find the best compatible version for a client request
    pub fn negotiate_version(&self, client_versions: &[Version]) -> Option<Version> {
        // Find the newest version that both client and server support
        for server_version in &self.supported_versions {
            if client_versions.contains(server_version) {
                return Some(server_version.clone());
            }
        }

        None
    }

    /// Check compatibility between two versions
    pub fn check_compatibility(
        &self,
        client_version: &Version,
        server_version: &Version,
    ) -> VersionCompatibility {
        if client_version == server_version {
            return VersionCompatibility::Compatible;
        }

        // Check if versions are in the same year (considered compatible)
        if client_version.year == server_version.year {
            let warning = format!(
                "Version mismatch but compatible: client={client_version}, server={server_version}"
            );
            return VersionCompatibility::CompatibleWithWarnings(vec![warning]);
        }

        // Major version difference
        let reason =
            format!("Incompatible versions: client={client_version}, server={server_version}");
        VersionCompatibility::Incompatible(reason)
    }

    /// Get the minimum supported version
    pub fn minimum_version(&self) -> &Version {
        self.supported_versions.last().unwrap() // Last because sorted newest first
    }

    /// Get the maximum supported version  
    pub fn maximum_version(&self) -> &Version {
        &self.supported_versions[0] // First because sorted newest first
    }

    /// Check if a version requirement is satisfied
    pub fn satisfies_requirement(
        &self,
        version: &Version,
        requirement: &VersionRequirement,
    ) -> bool {
        match requirement {
            VersionRequirement::Exact(required) => version == required,
            VersionRequirement::Minimum(min) => version >= min,
            VersionRequirement::Maximum(max) => version <= max,
            VersionRequirement::Range(min, max) => version >= min && version <= max,
            VersionRequirement::Any(versions) => versions.contains(version),
        }
    }
}

impl Default for VersionManager {
    fn default() -> Self {
        Self::with_default_versions()
    }
}

impl VersionRequirement {
    /// Create an exact version requirement
    pub fn exact(version: Version) -> Self {
        Self::Exact(version)
    }

    /// Create a minimum version requirement
    pub fn minimum(version: Version) -> Self {
        Self::Minimum(version)
    }

    /// Create a maximum version requirement
    pub fn maximum(version: Version) -> Self {
        Self::Maximum(version)
    }

    /// Create a version range requirement
    ///
    /// # Errors
    ///
    /// Returns [`VersionError::InvalidRange`] if `min` is greater than `max`.
    pub fn range(min: Version, max: Version) -> Result<Self, VersionError> {
        if min > max {
            return Err(VersionError::InvalidRange(min, max));
        }
        Ok(Self::Range(min, max))
    }

    /// Create an "any of" requirement
    ///
    /// # Errors
    ///
    /// Returns [`VersionError::EmptyVersionList`] if the provided vector is empty.
    pub fn any(versions: Vec<Version>) -> Result<Self, VersionError> {
        if versions.is_empty() {
            return Err(VersionError::EmptyVersionList);
        }
        Ok(Self::Any(versions))
    }

    /// Check if a version satisfies this requirement
    pub fn is_satisfied_by(&self, version: &Version) -> bool {
        match self {
            Self::Exact(required) => version == required,
            Self::Minimum(min) => version >= min,
            Self::Maximum(max) => version <= max,
            Self::Range(min, max) => version >= min && version <= max,
            Self::Any(versions) => versions.contains(version),
        }
    }
}

/// Version-related errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum VersionError {
    /// Invalid version format
    #[error("Invalid version format: {0}")]
    InvalidFormat(String),
    /// Invalid year
    #[error("Invalid year: {0}")]
    InvalidYear(String),
    /// Invalid month
    #[error("Invalid month: {0} (must be 1-12)")]
    InvalidMonth(String),
    /// Invalid day
    #[error("Invalid day: {0} (must be 1-31)")]
    InvalidDay(String),
    /// No supported versions
    #[error("No supported versions provided")]
    NoSupportedVersions,
    /// Invalid version range
    #[error("Invalid version range: {0} > {1}")]
    InvalidRange(Version, Version),
    /// Empty version list
    #[error("Empty version list")]
    EmptyVersionList,
}

/// Utility functions for version management
pub mod utils {
    use super::*;

    /// Parse multiple versions from strings
    ///
    /// # Errors
    ///
    /// Returns [`VersionError`] if any version string cannot be parsed.
    pub fn parse_versions(version_strings: &[&str]) -> Result<Vec<Version>, VersionError> {
        version_strings.iter().map(|s| s.parse()).collect()
    }

    /// Find the newest version in a list
    pub fn newest_version(versions: &[Version]) -> Option<&Version> {
        versions.iter().max()
    }

    /// Find the oldest version in a list
    pub fn oldest_version(versions: &[Version]) -> Option<&Version> {
        versions.iter().min()
    }

    /// Check if all versions in a list are compatible with each other
    pub fn are_all_compatible(versions: &[Version]) -> bool {
        if versions.len() < 2 {
            return true;
        }

        let first = &versions[0];
        versions.iter().all(|v| first.is_compatible_with(v))
    }

    /// Get a human-readable description of version compatibility
    pub fn compatibility_description(compatibility: &VersionCompatibility) -> String {
        match compatibility {
            VersionCompatibility::Compatible => "Fully compatible".to_string(),
            VersionCompatibility::CompatibleWithWarnings(warnings) => {
                format!("Compatible with warnings: {}", warnings.join(", "))
            }
            VersionCompatibility::Incompatible(reason) => {
                format!("Incompatible: {reason}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_version_creation() {
        let version = Version::new(2025, 6, 18).unwrap();
        assert_eq!(version.year, 2025);
        assert_eq!(version.month, 6);
        assert_eq!(version.day, 18);

        // Invalid month should fail
        assert!(Version::new(2025, 13, 18).is_err());

        // Invalid day should fail
        assert!(Version::new(2025, 6, 32).is_err());
    }

    #[test]
    fn test_version_parsing() {
        let version: Version = "2025-06-18".parse().unwrap();
        assert_eq!(version, Version::new(2025, 6, 18).unwrap());

        // Invalid format should fail
        assert!("2025/06/18".parse::<Version>().is_err());
        assert!("invalid".parse::<Version>().is_err());
    }

    #[test]
    fn test_version_comparison() {
        let v1 = Version::new(2025, 6, 18).unwrap();
        let v2 = Version::new(2024, 11, 5).unwrap();
        let v3 = Version::new(2025, 6, 18).unwrap();

        assert!(v1 > v2);
        assert!(v1.is_newer_than(&v2));
        assert!(v2.is_older_than(&v1));
        assert_eq!(v1, v3);
    }

    #[test]
    fn test_version_compatibility() {
        let v1 = Version::new(2025, 6, 18).unwrap();
        let v2 = Version::new(2025, 12, 1).unwrap(); // Same year
        let v3 = Version::new(2024, 6, 18).unwrap(); // Different year

        assert!(v1.is_compatible_with(&v2));
        assert!(!v1.is_compatible_with(&v3));
    }

    #[test]
    fn test_version_manager() {
        let versions = vec![
            Version::new(2025, 6, 18).unwrap(),
            Version::new(2024, 11, 5).unwrap(),
        ];

        let manager = VersionManager::new(versions).unwrap();

        assert_eq!(
            manager.current_version(),
            &Version::new(2025, 6, 18).unwrap()
        );
        assert!(manager.is_version_supported(&Version::new(2024, 11, 5).unwrap()));
        assert!(!manager.is_version_supported(&Version::new(2023, 1, 1).unwrap()));
    }

    #[test]
    fn test_version_negotiation() {
        let manager = VersionManager::default();

        let client_versions = vec![
            Version::new(2024, 11, 5).unwrap(),
            Version::new(2025, 6, 18).unwrap(),
        ];

        let negotiated = manager.negotiate_version(&client_versions);
        assert_eq!(negotiated, Some(Version::new(2025, 6, 18).unwrap()));
    }

    #[test]
    fn test_version_requirements() {
        let version = Version::new(2025, 6, 18).unwrap();

        let exact_req = VersionRequirement::exact(version.clone());
        assert!(exact_req.is_satisfied_by(&version));

        let min_req = VersionRequirement::minimum(Version::new(2024, 1, 1).unwrap());
        assert!(min_req.is_satisfied_by(&version));

        let max_req = VersionRequirement::maximum(Version::new(2024, 1, 1).unwrap());
        assert!(!max_req.is_satisfied_by(&version));
    }

    #[test]
    fn test_compatibility_checking() {
        let manager = VersionManager::default();

        let v1 = Version::new(2025, 6, 18).unwrap();
        let v2 = Version::new(2025, 12, 1).unwrap();
        let v3 = Version::new(2024, 1, 1).unwrap();

        // Same year - compatible with warnings
        let compat = manager.check_compatibility(&v1, &v2);
        assert!(matches!(
            compat,
            VersionCompatibility::CompatibleWithWarnings(_)
        ));

        // Different year - incompatible
        let compat = manager.check_compatibility(&v1, &v3);
        assert!(matches!(compat, VersionCompatibility::Incompatible(_)));

        // Exact match - compatible
        let compat = manager.check_compatibility(&v1, &v1);
        assert_eq!(compat, VersionCompatibility::Compatible);
    }

    #[test]
    fn test_utils() {
        let versions = utils::parse_versions(&["2025-06-18", "2024-11-05"]).unwrap();
        assert_eq!(versions.len(), 2);

        let newest = utils::newest_version(&versions);
        assert_eq!(newest, Some(&Version::new(2025, 6, 18).unwrap()));

        let oldest = utils::oldest_version(&versions);
        assert_eq!(oldest, Some(&Version::new(2024, 11, 5).unwrap()));
    }

    // Property-based tests for comprehensive coverage
    proptest! {
        #[test]
        fn test_version_parse_roundtrip(
            year in 2020u16..2030u16,
            month in 1u8..=12u8,
            day in 1u8..=28u8, // Use 28 to avoid month-specific day validation
        ) {
            let version = Version::new(year, month, day)?;
            let string = version.to_date_string();
            let parsed = Version::from_date_string(&string)?;
            prop_assert_eq!(version, parsed);
        }

        #[test]
        fn test_version_comparison_transitive(
            y1 in 2020u16..2030u16,
            m1 in 1u8..=12u8,
            d1 in 1u8..=28u8,
            y2 in 2020u16..2030u16,
            m2 in 1u8..=12u8,
            d2 in 1u8..=28u8,
            y3 in 2020u16..2030u16,
            m3 in 1u8..=12u8,
            d3 in 1u8..=28u8,
        ) {
            let v1 = Version::new(y1, m1, d1)?;
            let v2 = Version::new(y2, m2, d2)?;
            let v3 = Version::new(y3, m3, d3)?;

            // Transitivity: if v1 < v2 and v2 < v3, then v1 < v3
            if v1 < v2 && v2 < v3 {
                prop_assert!(v1 < v3);
            }
        }

        #[test]
        fn test_version_compatibility_symmetric(
            year in 2020u16..2030u16,
            m1 in 1u8..=12u8,
            d1 in 1u8..=28u8,
            m2 in 1u8..=12u8,
            d2 in 1u8..=28u8,
        ) {
            let v1 = Version::new(year, m1, d1)?;
            let v2 = Version::new(year, m2, d2)?;

            // Same-year versions should be compatible in both directions
            prop_assert_eq!(v1.is_compatible_with(&v2), v2.is_compatible_with(&v1));
        }

        #[test]
        fn test_invalid_month_rejected(
            year in 2020u16..2030u16,
            month in 13u8..=255u8,
            day in 1u8..=28u8,
        ) {
            prop_assert!(Version::new(year, month, day).is_err());
        }

        #[test]
        fn test_invalid_day_rejected(
            year in 2020u16..2030u16,
            month in 1u8..=12u8,
            day in 32u8..=255u8,
        ) {
            prop_assert!(Version::new(year, month, day).is_err());
        }
    }
}
