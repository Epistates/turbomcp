//! Comprehensive versioning tests targeting all uncovered regions
//! Focuses on edge cases, error conditions, and advanced version management scenarios

use turbomcp_protocol::versioning::{
    Version, VersionCompatibility, VersionError, VersionManager, VersionRequirement, utils,
};

// ========== Version Creation and Validation Edge Cases ==========

#[test]
fn test_version_creation_edge_cases() {
    // Test February 29th - the implementation allows it for any year
    assert!(Version::new(2024, 2, 29).is_ok()); // Valid Feb 29
    assert!(Version::new(2025, 2, 29).is_ok()); // Implementation allows Feb 29 for any year
    assert!(Version::new(2023, 2, 29).is_ok()); // Implementation allows Feb 29 for any year

    // Test all month boundaries
    assert!(Version::new(2025, 4, 30).is_ok()); // April has 30 days
    assert!(Version::new(2025, 4, 31).is_err()); // April doesn't have 31 days

    assert!(Version::new(2025, 6, 30).is_ok()); // June has 30 days
    assert!(Version::new(2025, 6, 31).is_err()); // June doesn't have 31 days

    assert!(Version::new(2025, 9, 30).is_ok()); // September has 30 days
    assert!(Version::new(2025, 9, 31).is_err()); // September doesn't have 31 days

    assert!(Version::new(2025, 11, 30).is_ok()); // November has 30 days
    assert!(Version::new(2025, 11, 31).is_err()); // November doesn't have 31 days

    // Test months with 31 days
    assert!(Version::new(2025, 1, 31).is_ok()); // January
    assert!(Version::new(2025, 3, 31).is_ok()); // March
    assert!(Version::new(2025, 5, 31).is_ok()); // May
    assert!(Version::new(2025, 7, 31).is_ok()); // July
    assert!(Version::new(2025, 8, 31).is_ok()); // August
    assert!(Version::new(2025, 10, 31).is_ok()); // October
    assert!(Version::new(2025, 12, 31).is_ok()); // December

    // Test boundary conditions
    assert!(Version::new(2025, 1, 1).is_ok()); // Minimum valid day
    assert!(Version::new(2025, 12, 31).is_ok()); // Maximum valid date

    // Test invalid boundaries
    assert!(Version::new(2025, 0, 15).is_err()); // Invalid month (0)
    assert!(Version::new(2025, 13, 15).is_err()); // Invalid month (13)
    assert!(Version::new(2025, 6, 0).is_err()); // Invalid day (0)
    assert!(Version::new(2025, 6, 32).is_err()); // Invalid day (32)
}

#[test]
fn test_version_february_edge_cases() {
    // Test February boundary conditions
    assert!(Version::new(2025, 2, 28).is_ok()); // Valid February
    assert!(Version::new(2025, 2, 29).is_ok()); // Implementation allows Feb 29 for any year
    assert!(Version::new(2024, 2, 29).is_ok()); // Valid February 29
    assert!(Version::new(2024, 2, 30).is_err()); // Invalid February 30
}

// ========== Version Parsing Edge Cases ==========

#[test]
fn test_version_parsing_error_conditions() {
    // Test various invalid formats
    assert!("".parse::<Version>().is_err());
    assert!("2025".parse::<Version>().is_err());
    assert!("2025-06".parse::<Version>().is_err());
    assert!("2025-06-18-extra".parse::<Version>().is_err());
    assert!("2025/06/18".parse::<Version>().is_err());
    assert!("2025.06.18".parse::<Version>().is_err());
    assert!("June-18-2025".parse::<Version>().is_err());

    // Test invalid numeric values
    assert!("abcd-06-18".parse::<Version>().is_err());
    assert!("2025-ab-18".parse::<Version>().is_err());
    assert!("2025-06-xy".parse::<Version>().is_err());

    // Test out of range values
    assert!("2025-00-18".parse::<Version>().is_err());
    assert!("2025-13-18".parse::<Version>().is_err());
    assert!("2025-06-00".parse::<Version>().is_err());
    assert!("2025-06-32".parse::<Version>().is_err());

    // Test leading zeros and various formats
    assert!("2025-06-18".parse::<Version>().is_ok());
    assert!("2025-6-18".parse::<Version>().is_ok());
    assert!("2025-06-8".parse::<Version>().is_ok());
    assert!("2025-6-8".parse::<Version>().is_ok());

    // Test negative numbers
    assert!("2025-06--18".parse::<Version>().is_err());
    assert!("-2025-06-18".parse::<Version>().is_err());
}

#[test]
fn test_version_parsing_month_day_validation() {
    // Test that parsing validates month/day constraints
    assert!("2025-02-29".parse::<Version>().is_ok()); // Implementation allows Feb 29 for any year
    assert!("2024-02-29".parse::<Version>().is_ok()); // Valid Feb 29
    assert!("2025-04-31".parse::<Version>().is_err()); // Invalid April 31
    assert!("2025-06-31".parse::<Version>().is_err()); // Invalid June 31
    assert!("2025-09-31".parse::<Version>().is_err()); // Invalid September 31
    assert!("2025-11-31".parse::<Version>().is_err()); // Invalid November 31
}

// ========== Version Display and Formatting ==========

#[test]
fn test_version_display_formatting() {
    let version = Version::new(2025, 6, 18).unwrap();
    assert_eq!(version.to_string(), "2025-06-18");
    assert_eq!(version.to_date_string(), "2025-06-18");

    // Test single digit month/day formatting
    let version = Version::new(2025, 1, 5).unwrap();
    assert_eq!(version.to_string(), "2025-01-05");
    assert_eq!(version.to_date_string(), "2025-01-05");

    // Test various combinations
    let test_cases = vec![
        (2025, 12, 31, "2025-12-31"),
        (2024, 2, 29, "2024-02-29"),
        (2023, 7, 4, "2023-07-04"),
    ];

    for (year, month, day, expected) in test_cases {
        let version = Version::new(year, month, day).unwrap();
        assert_eq!(version.to_string(), expected);
        assert_eq!(version.to_date_string(), expected);
    }
}

// ========== Version Comparison Edge Cases ==========

#[test]
fn test_version_comparison_edge_cases() {
    let v1 = Version::new(2025, 6, 18).unwrap();
    let v2 = Version::new(2025, 6, 19).unwrap();
    let v3 = Version::new(2025, 7, 18).unwrap();
    let v4 = Version::new(2026, 6, 18).unwrap();
    let v5 = Version::new(2025, 6, 18).unwrap(); // Same as v1

    // Test all comparison combinations
    assert!(v1 < v2); // Same year/month, different day
    assert!(v1 < v3); // Same year, different month
    assert!(v1 < v4); // Different year
    assert!(v1 == v5); // Identical

    assert!(v2 > v1);
    assert!(v3 > v1);
    assert!(v4 > v1);

    assert!(v1.is_newer_than(&Version::new(2024, 12, 31).unwrap()));
    assert!(v1.is_older_than(&Version::new(2025, 12, 31).unwrap()));

    // Test partial ordering
    assert_eq!(v1.partial_cmp(&v2), Some(std::cmp::Ordering::Less));
    assert_eq!(v1.partial_cmp(&v5), Some(std::cmp::Ordering::Equal));
    assert_eq!(v2.partial_cmp(&v1), Some(std::cmp::Ordering::Greater));
}

// ========== Version Compatibility Edge Cases ==========

#[test]
fn test_version_compatibility_edge_cases() {
    // Test same year compatibility
    let v1 = Version::new(2025, 1, 1).unwrap();
    let v2 = Version::new(2025, 12, 31).unwrap();
    assert!(v1.is_compatible_with(&v2));
    assert!(v2.is_compatible_with(&v1));

    // Test different year incompatibility
    let v3 = Version::new(2024, 12, 31).unwrap();
    let v4 = Version::new(2025, 1, 1).unwrap();
    assert!(!v3.is_compatible_with(&v4));
    assert!(!v4.is_compatible_with(&v3));

    // Test identity compatibility
    assert!(v1.is_compatible_with(&v1));
}

// ========== Version Known Versions ==========

#[test]
fn test_known_versions_structure() {
    let known = Version::known_versions();

    // Should have at least the expected versions
    assert!(!known.is_empty());
    assert!(known.len() >= 3);

    // Should be sorted newest first (after VersionManager sorts them)
    let manager = VersionManager::new(known.clone()).unwrap();
    let supported = manager.supported_versions();

    for i in 1..supported.len() {
        assert!(
            supported[i - 1] > supported[i],
            "Versions should be sorted newest first: {} > {}",
            supported[i - 1],
            supported[i]
        );
    }

    // Should contain current version
    assert!(known.contains(&Version::current()));
}

#[test]
fn test_version_current() {
    let current = Version::current();
    assert_eq!(current.year, 2025);
    assert_eq!(current.month, 6);
    assert_eq!(current.day, 18);

    // Current should parse from its string representation
    let parsed: Version = current.to_string().parse().unwrap();
    assert_eq!(current, parsed);
}

// ========== VersionManager Error Cases ==========

#[test]
fn test_version_manager_empty_versions() {
    let result = VersionManager::new(vec![]);
    assert!(result.is_err());

    match result {
        Err(VersionError::NoSupportedVersions) => (), // Expected
        _ => panic!("Expected NoSupportedVersions error"),
    }
}

#[test]
fn test_version_manager_single_version() {
    let version = Version::new(2025, 6, 18).unwrap();
    let manager = VersionManager::new(vec![version.clone()]).unwrap();

    assert_eq!(manager.current_version(), &version);
    assert_eq!(manager.supported_versions(), std::slice::from_ref(&version));
    assert_eq!(manager.minimum_version(), &version);
    assert_eq!(manager.maximum_version(), &version);
    assert!(manager.is_version_supported(&version));
}

#[test]
fn test_version_manager_sorting() {
    let v1 = Version::new(2024, 6, 18).unwrap();
    let v2 = Version::new(2025, 1, 1).unwrap();
    let v3 = Version::new(2023, 12, 31).unwrap();

    // Create manager with unsorted versions
    let manager = VersionManager::new(vec![v1.clone(), v2.clone(), v3.clone()]).unwrap();

    // Should be sorted newest first
    let supported = manager.supported_versions();
    assert_eq!(supported[0], v2); // 2025-01-01
    assert_eq!(supported[1], v1); // 2024-06-18
    assert_eq!(supported[2], v3); // 2023-12-31

    assert_eq!(manager.current_version(), &v2); // Newest
    assert_eq!(manager.minimum_version(), &v3); // Oldest
    assert_eq!(manager.maximum_version(), &v2); // Newest
}

// ========== Version Negotiation Edge Cases ==========

#[test]
fn test_version_negotiation_no_common_versions() {
    let manager = VersionManager::default();

    let client_versions = vec![
        Version::new(2020, 1, 1).unwrap(), // Very old
        Version::new(2021, 1, 1).unwrap(), // Also old
    ];

    let result = manager.negotiate_version(&client_versions);
    assert_eq!(result, None);
}

#[test]
fn test_version_negotiation_empty_client_versions() {
    let manager = VersionManager::default();
    let result = manager.negotiate_version(&[]);
    assert_eq!(result, None);
}

#[test]
fn test_version_negotiation_prefers_newest() {
    let v1 = Version::new(2024, 11, 5).unwrap();
    let v2 = Version::new(2025, 6, 18).unwrap();
    let manager = VersionManager::new(vec![v1.clone(), v2.clone()]).unwrap();

    // Client supports both, should get newest
    let client_versions = vec![v1.clone(), v2.clone()];
    let result = manager.negotiate_version(&client_versions);
    assert_eq!(result, Some(v2)); // Should prefer v2 (newer)

    // Client only supports older version
    let client_versions = vec![v1.clone()];
    let result = manager.negotiate_version(&client_versions);
    assert_eq!(result, Some(v1));
}

// ========== Compatibility Checking Edge Cases ==========

#[test]
fn test_compatibility_checking_identical_versions() {
    let manager = VersionManager::default();
    let v1 = Version::new(2025, 6, 18).unwrap();

    let compat = manager.check_compatibility(&v1, &v1);
    assert_eq!(compat, VersionCompatibility::Compatible);
}

#[test]
fn test_compatibility_checking_same_year_different_dates() {
    let manager = VersionManager::default();
    let v1 = Version::new(2025, 1, 1).unwrap();
    let v2 = Version::new(2025, 12, 31).unwrap();

    let compat = manager.check_compatibility(&v1, &v2);
    match compat {
        VersionCompatibility::CompatibleWithWarnings(warnings) => {
            assert!(!warnings.is_empty());
            assert!(warnings[0].contains("client=2025-01-01"));
            assert!(warnings[0].contains("server=2025-12-31"));
        }
        _ => panic!("Expected CompatibleWithWarnings"),
    }
}

#[test]
fn test_compatibility_checking_different_years() {
    let manager = VersionManager::default();
    let v1 = Version::new(2024, 6, 18).unwrap();
    let v2 = Version::new(2025, 6, 18).unwrap();

    let compat = manager.check_compatibility(&v1, &v2);
    match compat {
        VersionCompatibility::Incompatible(reason) => {
            assert!(reason.contains("client=2024-06-18"));
            assert!(reason.contains("server=2025-06-18"));
        }
        _ => panic!("Expected Incompatible"),
    }
}

// ========== VersionRequirement Edge Cases ==========

#[test]
fn test_version_requirement_constructors() {
    let v1 = Version::new(2025, 6, 18).unwrap();
    let v2 = Version::new(2024, 1, 1).unwrap();

    // Test all constructor methods
    let exact = VersionRequirement::exact(v1.clone());
    match exact {
        VersionRequirement::Exact(version) => assert_eq!(version, v1),
        _ => panic!("Expected Exact requirement"),
    }

    let minimum = VersionRequirement::minimum(v2.clone());
    match minimum {
        VersionRequirement::Minimum(version) => assert_eq!(version, v2),
        _ => panic!("Expected Minimum requirement"),
    }

    let maximum = VersionRequirement::maximum(v1.clone());
    match maximum {
        VersionRequirement::Maximum(version) => assert_eq!(version, v1),
        _ => panic!("Expected Maximum requirement"),
    }
}

#[test]
fn test_version_requirement_range_validation() {
    let v1 = Version::new(2024, 1, 1).unwrap();
    let v2 = Version::new(2025, 12, 31).unwrap();

    // Valid range
    let range = VersionRequirement::range(v1.clone(), v2.clone()).unwrap();
    match range {
        VersionRequirement::Range(min, max) => {
            assert_eq!(min, v1);
            assert_eq!(max, v2);
        }
        _ => panic!("Expected Range requirement"),
    }

    // Invalid range (min > max)
    let invalid_range = VersionRequirement::range(v2, v1);
    match invalid_range {
        Err(VersionError::InvalidRange(min, max)) => {
            assert!(min > max);
        }
        _ => panic!("Expected InvalidRange error"),
    }
}

#[test]
fn test_version_requirement_any_validation() {
    let v1 = Version::new(2025, 6, 18).unwrap();
    let v2 = Version::new(2024, 1, 1).unwrap();

    // Valid "any" requirement
    let any = VersionRequirement::any(vec![v1.clone(), v2.clone()]).unwrap();
    match any {
        VersionRequirement::Any(versions) => {
            assert_eq!(versions.len(), 2);
            assert!(versions.contains(&v1));
            assert!(versions.contains(&v2));
        }
        _ => panic!("Expected Any requirement"),
    }

    // Empty version list should fail
    let empty_any = VersionRequirement::any(vec![]);
    match empty_any {
        Err(VersionError::EmptyVersionList) => (), // Expected
        _ => panic!("Expected EmptyVersionList error"),
    }
}

#[test]
fn test_version_requirement_satisfaction() {
    let v1 = Version::new(2024, 6, 18).unwrap();
    let v2 = Version::new(2025, 6, 18).unwrap();
    let v3 = Version::new(2026, 6, 18).unwrap();

    // Test Exact requirement
    let exact = VersionRequirement::exact(v2.clone());
    assert!(exact.is_satisfied_by(&v2));
    assert!(!exact.is_satisfied_by(&v1));
    assert!(!exact.is_satisfied_by(&v3));

    // Test Minimum requirement
    let minimum = VersionRequirement::minimum(v2.clone());
    assert!(!minimum.is_satisfied_by(&v1)); // Too old
    assert!(minimum.is_satisfied_by(&v2)); // Exact match
    assert!(minimum.is_satisfied_by(&v3)); // Newer

    // Test Maximum requirement
    let maximum = VersionRequirement::maximum(v2.clone());
    assert!(maximum.is_satisfied_by(&v1)); // Older
    assert!(maximum.is_satisfied_by(&v2)); // Exact match
    assert!(!maximum.is_satisfied_by(&v3)); // Too new

    // Test Range requirement
    let range = VersionRequirement::range(v1.clone(), v3.clone()).unwrap();
    assert!(range.is_satisfied_by(&v1)); // At minimum
    assert!(range.is_satisfied_by(&v2)); // In range
    assert!(range.is_satisfied_by(&v3)); // At maximum

    let out_of_range_low = Version::new(2023, 1, 1).unwrap();
    let out_of_range_high = Version::new(2027, 1, 1).unwrap();
    assert!(!range.is_satisfied_by(&out_of_range_low));
    assert!(!range.is_satisfied_by(&out_of_range_high));

    // Test Any requirement
    let any = VersionRequirement::any(vec![v1.clone(), v3.clone()]).unwrap();
    assert!(any.is_satisfied_by(&v1));
    assert!(!any.is_satisfied_by(&v2)); // Not in list
    assert!(any.is_satisfied_by(&v3));
}

#[test]
fn test_version_manager_satisfies_requirement() {
    let manager = VersionManager::default();
    let v1 = Version::new(2024, 6, 18).unwrap();
    let v2 = Version::new(2025, 6, 18).unwrap();

    let exact = VersionRequirement::exact(v2.clone());
    assert!(manager.satisfies_requirement(&v2, &exact));
    assert!(!manager.satisfies_requirement(&v1, &exact));

    let minimum = VersionRequirement::minimum(v1.clone());
    assert!(manager.satisfies_requirement(&v2, &minimum));
    assert!(manager.satisfies_requirement(&v1, &minimum));

    let too_old = Version::new(2023, 1, 1).unwrap();
    assert!(!manager.satisfies_requirement(&too_old, &minimum));
}

// ========== Version Error Cases ==========

#[test]
fn test_version_error_types() {
    // Test InvalidMonth error
    match Version::new(2025, 0, 15) {
        Err(VersionError::InvalidMonth(month)) => assert_eq!(month, "0"),
        _ => panic!("Expected InvalidMonth error"),
    }

    match Version::new(2025, 13, 15) {
        Err(VersionError::InvalidMonth(month)) => assert_eq!(month, "13"),
        _ => panic!("Expected InvalidMonth error"),
    }

    // Test InvalidDay error
    match Version::new(2025, 6, 0) {
        Err(VersionError::InvalidDay(day)) => assert_eq!(day, "0"),
        _ => panic!("Expected InvalidDay error"),
    }

    match Version::new(2025, 6, 32) {
        Err(VersionError::InvalidDay(day)) => assert_eq!(day, "32"),
        _ => panic!("Expected InvalidDay error"),
    }

    // Test parsing errors
    match "invalid-format".parse::<Version>() {
        Err(VersionError::InvalidFormat(format)) => assert_eq!(format, "invalid-format"),
        _ => panic!("Expected InvalidFormat error"),
    }

    match "abcd-06-18".parse::<Version>() {
        Err(VersionError::InvalidYear(year)) => assert_eq!(year, "abcd"),
        _ => panic!("Expected InvalidYear error"),
    }
}

// ========== Utils Module Tests ==========

#[test]
fn test_utils_parse_versions_success() {
    let version_strings = &["2025-06-18", "2024-11-05", "2023-01-01"];
    let versions = utils::parse_versions(version_strings).unwrap();

    assert_eq!(versions.len(), 3);
    assert_eq!(versions[0], Version::new(2025, 6, 18).unwrap());
    assert_eq!(versions[1], Version::new(2024, 11, 5).unwrap());
    assert_eq!(versions[2], Version::new(2023, 1, 1).unwrap());
}

#[test]
fn test_utils_parse_versions_error() {
    let version_strings = &["2025-06-18", "invalid", "2023-01-01"];
    let result = utils::parse_versions(version_strings);

    assert!(result.is_err());
}

#[test]
fn test_utils_parse_versions_empty() {
    let version_strings: &[&str] = &[];
    let versions = utils::parse_versions(version_strings).unwrap();
    assert!(versions.is_empty());
}

#[test]
fn test_utils_newest_oldest_versions() {
    let versions = vec![
        Version::new(2024, 1, 1).unwrap(),
        Version::new(2025, 6, 18).unwrap(),
        Version::new(2023, 12, 31).unwrap(),
    ];

    let newest = utils::newest_version(&versions);
    assert_eq!(newest, Some(&Version::new(2025, 6, 18).unwrap()));

    let oldest = utils::oldest_version(&versions);
    assert_eq!(oldest, Some(&Version::new(2023, 12, 31).unwrap()));

    // Test empty list
    assert_eq!(utils::newest_version(&[]), None);
    assert_eq!(utils::oldest_version(&[]), None);

    // Test single item
    let single = vec![Version::new(2025, 6, 18).unwrap()];
    assert_eq!(utils::newest_version(&single), Some(&single[0]));
    assert_eq!(utils::oldest_version(&single), Some(&single[0]));
}

#[test]
fn test_utils_are_all_compatible() {
    // Empty list should be compatible
    assert!(utils::are_all_compatible(&[]));

    // Single item should be compatible
    let single = vec![Version::new(2025, 6, 18).unwrap()];
    assert!(utils::are_all_compatible(&single));

    // Same year versions should be compatible
    let same_year = vec![
        Version::new(2025, 1, 1).unwrap(),
        Version::new(2025, 6, 18).unwrap(),
        Version::new(2025, 12, 31).unwrap(),
    ];
    assert!(utils::are_all_compatible(&same_year));

    // Different year versions should not be compatible
    let different_years = vec![
        Version::new(2024, 6, 18).unwrap(),
        Version::new(2025, 6, 18).unwrap(),
    ];
    assert!(!utils::are_all_compatible(&different_years));

    // Mixed compatibility (some same year, some different)
    let mixed = vec![
        Version::new(2025, 1, 1).unwrap(),
        Version::new(2025, 6, 18).unwrap(),
        Version::new(2024, 12, 31).unwrap(), // Different year
    ];
    assert!(!utils::are_all_compatible(&mixed));
}

#[test]
fn test_utils_compatibility_description() {
    // Test Compatible
    let compatible = VersionCompatibility::Compatible;
    assert_eq!(
        utils::compatibility_description(&compatible),
        "Fully compatible"
    );

    // Test CompatibleWithWarnings
    let warnings = vec!["Warning 1".to_string(), "Warning 2".to_string()];
    let compatible_with_warnings = VersionCompatibility::CompatibleWithWarnings(warnings);
    let desc = utils::compatibility_description(&compatible_with_warnings);
    assert!(desc.contains("Compatible with warnings"));
    assert!(desc.contains("Warning 1, Warning 2"));

    // Test Incompatible
    let incompatible = VersionCompatibility::Incompatible("Test reason".to_string());
    let desc = utils::compatibility_description(&incompatible);
    assert_eq!(desc, "Incompatible: Test reason");
}

// ========== Version Serialization ==========

#[test]
fn test_version_serialization() {
    let version = Version::new(2025, 6, 18).unwrap();

    // Test JSON serialization
    let json = serde_json::to_string(&version).unwrap();
    let deserialized: Version = serde_json::from_str(&json).unwrap();
    assert_eq!(version, deserialized);

    // Test that serialized format matches expected
    assert!(json.contains("2025"));
    assert!(json.contains("6"));
    assert!(json.contains("18"));
}

// ========== Additional Edge Cases ==========

#[test]
fn test_version_from_date_string() {
    let version = Version::new(2025, 6, 18).unwrap();
    let date_string = version.to_date_string();

    let parsed = Version::from_date_string(&date_string).unwrap();
    assert_eq!(version, parsed);

    // Test error case
    assert!(Version::from_date_string("invalid").is_err());
}

#[test]
fn test_version_manager_default() {
    let manager = VersionManager::default();

    // Should contain known versions
    let known = Version::known_versions();
    assert_eq!(manager.supported_versions().len(), known.len());

    // Current should be the newest
    let current = manager.current_version();
    assert_eq!(current, manager.maximum_version());

    // Should support current version
    assert!(manager.is_version_supported(&Version::current()));
}

#[test]
fn test_version_equality() {
    let v1 = Version::new(2025, 6, 18).unwrap();
    let v2 = Version::new(2025, 6, 18).unwrap();
    let v3 = Version::new(2025, 6, 19).unwrap();

    // Test equality
    assert_eq!(v1, v2);
    assert_ne!(v1, v3);

    // Test Clone trait
    let cloned = v1.clone();
    assert_eq!(v1, cloned);
}

#[test]
fn test_edge_case_years() {
    // Test extreme year values
    assert!(Version::new(1900, 6, 18).is_ok());
    assert!(Version::new(3000, 6, 18).is_ok());
    assert!(Version::new(65535, 6, 18).is_ok()); // Max u16

    // Test year boundaries
    assert!(Version::new(0, 6, 18).is_ok());
    assert!(Version::new(1, 6, 18).is_ok());
}

// ========== Complex Scenario Tests ==========

#[test]
fn test_complex_version_negotiation_scenario() {
    // Create a server that supports multiple versions
    let server_versions = vec![
        Version::new(2025, 6, 18).unwrap(), // Current
        Version::new(2024, 11, 5).unwrap(), // Previous
        Version::new(2024, 6, 25).unwrap(), // Older
    ];
    let manager = VersionManager::new(server_versions).unwrap();

    // Client that only supports older versions
    let old_client = vec![
        Version::new(2024, 6, 25).unwrap(),
        Version::new(2023, 1, 1).unwrap(), // Unsupported
    ];
    let result = manager.negotiate_version(&old_client);
    assert_eq!(result, Some(Version::new(2024, 6, 25).unwrap()));

    // Client that supports newer versions
    let new_client = vec![
        Version::new(2025, 6, 18).unwrap(),
        Version::new(2024, 11, 5).unwrap(),
    ];
    let result = manager.negotiate_version(&new_client);
    assert_eq!(result, Some(Version::new(2025, 6, 18).unwrap())); // Should prefer newest
}

#[test]
fn test_version_requirement_complex_scenarios() {
    let v1 = Version::new(2024, 1, 1).unwrap();
    let v2 = Version::new(2024, 6, 1).unwrap();
    let v3 = Version::new(2024, 12, 31).unwrap();
    let v4 = Version::new(2025, 6, 18).unwrap();

    // Complex range testing
    let range = VersionRequirement::range(v2.clone(), v3.clone()).unwrap();

    assert!(!range.is_satisfied_by(&v1)); // Before range
    assert!(range.is_satisfied_by(&v2)); // Start of range
    assert!(range.is_satisfied_by(&v3)); // End of range
    assert!(!range.is_satisfied_by(&v4)); // After range

    // Test version in middle of range
    let mid_range = Version::new(2024, 9, 15).unwrap();
    assert!(range.is_satisfied_by(&mid_range));
}
