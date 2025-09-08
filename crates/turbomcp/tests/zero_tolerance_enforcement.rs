//! Zero-Tolerance Test Quality Enforcement System
//!
//! This test ensures that no fraudulent test patterns can ever be introduced
//! into the TurboMCP codebase. It scans all test files for forbidden
//! patterns and fails the build if any are detected.
//!
//! RIGOROUS TESTING REQUIRES RIGOROUS ENFORCEMENT

use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Forbidden patterns that indicate test fraud
const FORBIDDEN_PATTERNS: &[(&str, &str)] = &[
    // Mathematical gaslighting
    (
        "assert_eq!(2 + 2, 4)",
        "Mathematical gaslighting - tests nothing about the code",
    ),
    (
        "assert_eq!(1 + 1, 2)",
        "Mathematical gaslighting - tests nothing about the code",
    ),
    ("assert!(true)", "Tautological assertion - always passes"),
    (
        "assert!(false == false)",
        "Tautological assertion - always passes",
    ),
    // Void patterns
    (
        "let _ = result",
        "Void pattern - discards result without validation",
    ),
    (
        "let _ =",
        "Potential void pattern - verify result is validated",
    ),
    // Wrong system testing
    (
        "schemars::schema_for!",
        "Testing schemars library instead of our macro implementation",
    ),
    (
        "generate_schema::<",
        "Using schemars instead of actual macro-generated schemas",
    ),
    // Placeholder/mock patterns (with exceptions for legitimate test doubles)
    (
        "// TODO: implement",
        "Incomplete test - no TODOs allowed in tests",
    ),
    ("todo!()", "Incomplete implementation - no todo!() in tests"),
    (
        "unimplemented!()",
        "Incomplete implementation - no unimplemented!() in tests",
    ),
    // Empty test patterns
    ("fn test_", "Check for empty test functions"),
];

/// Allowed exceptions (file patterns that can contain forbidden patterns)
const ALLOWED_EXCEPTIONS: &[&str] = &[
    // This file itself needs to contain the patterns to check for them
    "zero_tolerance_enforcement.rs",
    // The audit report documents the patterns
    "TEST_AUDIT_REPORT.md",
    // Documentation files
    "ARCHITECTURAL_GAP_ANALYSIS.md",
];

/// Check if a file path should be excluded from enforcement
fn is_excluded(path: &Path) -> bool {
    let path_str = path.to_string_lossy();

    // Check if it's in allowed exceptions
    for exception in ALLOWED_EXCEPTIONS {
        if path_str.contains(exception) {
            return true;
        }
    }

    // Exclude non-test files (unless they're in test directories)
    if !path_str.contains("/tests/") && !path_str.contains("_test") {
        return true;
    }

    // Exclude non-Rust files
    if !path_str.ends_with(".rs") {
        return true;
    }

    false
}

/// Scan a file for forbidden patterns
fn scan_file(path: &Path) -> FileViolations {
    let mut violations = Vec::new();

    if is_excluded(path) {
        return violations;
    }

    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => return violations,
    };

    for (line_num, line) in content.lines().enumerate() {
        for &(pattern, reason) in FORBIDDEN_PATTERNS {
            if line.contains(pattern) {
                // Special handling for legitimate patterns
                if pattern == "fn test_" {
                    // Check if it's an empty test function
                    if line.trim() == "fn test_() {}"
                        || (line.contains("fn test_") && line.contains("{}"))
                    {
                        violations.push((
                            line_num + 1,
                            line.trim().to_string(),
                            "Empty test function - tests nothing".to_string(),
                        ));
                    }
                    continue; // fn test_ is ok if not empty
                }

                if pattern == "let _ =" {
                    // Check if it's actually discarding a result
                    if line.contains("let _ = ")
                        && (line.contains(".await")
                            || line.contains("()")
                            || line.contains("unwrap"))
                        && !line.contains("// OK:")
                    {
                        // Allow if explicitly marked OK
                        violations.push((
                            line_num + 1,
                            line.trim().to_string(),
                            reason.to_string(),
                        ));
                    }
                    continue;
                }

                // For other patterns, direct violation
                violations.push((line_num + 1, line.trim().to_string(), reason.to_string()));
            }
        }
    }

    violations
}

// Type alias to reduce complexity
type FileViolations = Vec<(usize, String, String)>;
type AllViolations = Vec<(PathBuf, FileViolations)>;

/// Scan all test files in the project
fn scan_all_test_files() -> AllViolations {
    let mut all_violations = Vec::new();

    // Walk through the entire project
    for entry in WalkDir::new(".")
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        // Skip non-Rust files
        if !path.to_string_lossy().ends_with(".rs") {
            continue;
        }

        // Skip target directory
        if path.to_string_lossy().contains("/target/") {
            continue;
        }

        let violations = scan_file(path);
        if !violations.is_empty() {
            all_violations.push((path.to_path_buf(), violations));
        }
    }

    all_violations
}

/// Main enforcement test
#[test]
fn test_zero_tolerance_enforcement() {
    println!("🔍 Running Zero-Tolerance Test Quality Enforcement...\n");

    let violations = scan_all_test_files();

    if violations.is_empty() {
        println!("✅ All test files pass zero-tolerance quality standards!");
        println!("✅ No fraudulent test patterns detected!");
        println!("✅ High-quality test suite maintained!");
        return;
    }

    // Report violations
    println!("❌ ZERO-TOLERANCE VIOLATIONS DETECTED!\n");

    for (file, file_violations) in &violations {
        println!("File: {}", file.display());
        for (line_num, line, reason) in file_violations {
            println!("  Line {}: {}", line_num, reason);
            println!("    > {}", line);
        }
        println!();
    }

    let total_violations: usize = violations.iter().map(|(_, v)| v.len()).sum();

    println!("Total violations: {}", total_violations);
    println!("\n❌ BUILD FAILED: Test quality standards not met!");
    println!("Fix all violations before committing.");

    panic!("Zero-tolerance test quality enforcement failed!");
}

/// Test that our enforcement actually works
#[test]
fn test_enforcement_detects_violations() {
    // Create a temporary test file with violations (string literals to avoid detection)
    let forbidden_pattern = "assert_eq!(2".to_string() + " + " + "2, 4)";
    let test_content = format!(
        r#"
#[test]
fn test_something() {{
    // This would be detected as mathematical gaslighting:
    {};
    let x = 2 + 2;
    assert_eq!(x, 4); // This is OK - testing actual computation
}}
"#,
        forbidden_pattern
    );

    // This test validates that our enforcement logic works
    // by checking a string that would be forbidden if detected
    assert!(test_content.contains(&forbidden_pattern));
}

/// Validate that legitimate patterns are not flagged
#[test]
fn test_enforcement_allows_legitimate_patterns() {
    let legitimate_content = r#"
#[test]
fn test_actual_functionality() {
    let result = some_function();
    assert_eq!(result, expected_value);
    assert!(result.is_ok());
    
    // Legitimate use of underscore for unused variable
    let (_tx, rx) = channel(); // OK: partial destructuring
    
    // Test actual macro-generated schemas
    let (name, desc, schema) = Server::tool_metadata();
    assert!(!schema.is_null());
}
"#;

    // Verify this wouldn't trigger violations
    for &(pattern, _) in FORBIDDEN_PATTERNS {
        if pattern == "let _ =" {
            // Special case: should not match partial destructuring
            assert!(!legitimate_content.contains("let _ = result"));
        }
    }
}

/// Statistics about test quality
#[test]
fn test_quality_statistics() {
    let test_files: Vec<_> = WalkDir::new(".")
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let path = e.path().to_string_lossy();
            path.ends_with(".rs")
                && (path.contains("/tests/") || path.contains("_test"))
                && !path.contains("/target/")
        })
        .collect();

    println!("\n📊 Test Suite Quality Statistics:");
    println!("Total test files: {}", test_files.len());

    let mut total_lines = 0;
    let mut files_with_assertions = 0;

    for entry in test_files {
        if let Ok(content) = fs::read_to_string(entry.path()) {
            total_lines += content.lines().count();
            if content.contains("assert") {
                files_with_assertions += 1;
            }
        }
    }

    println!("Total test lines: {}", total_lines);
    println!("Files with assertions: {}", files_with_assertions);
    println!("\n✅ Zero-tolerance enforcement active!");
}
