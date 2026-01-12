//! Handler name validation - prevents injection attacks using `syn::Ident`
//!
//! Validates handler names are valid Rust identifiers, preventing:
//! - Path traversal (`../etc/passwd`)
//! - Injection attempts (`handler"; DROP TABLE`)
//! - Reserved keywords (`async`, `impl`)
//! - Reserved system names (`admin`, `initialize`)

use crate::ServerResult;

#[cfg(feature = "security")]
const RESERVED_HANDLER_NAMES: &[&str] = &[
    "initialize",
    "initialized",
    "shutdown",
    "ping",
    "pong",
    "health",
    "status",
    "admin",
    "root",
    "system",
    "sudo",
    "su",
    "superuser",
    "internal",
    "private",
    "protected",
    "__init__",
    "__main__",
];

#[cfg(feature = "security")]
const MAX_HANDLER_NAME_LENGTH: usize = 128;

/// Validate handler name: must be valid Rust identifier, not a keyword or reserved name
#[cfg(feature = "security")]
pub fn validate_handler_name(name: &str) -> ServerResult<()> {
    use crate::McpError;

    if name.is_empty() {
        return Err(McpError::handler(
            "Handler name cannot be empty".to_string(),
        ));
    }

    if name.len() > MAX_HANDLER_NAME_LENGTH {
        return Err(McpError::handler(format!(
            "Handler name '{}...' exceeds maximum length of {} characters",
            &name[..50.min(name.len())],
            MAX_HANDLER_NAME_LENGTH
        )));
    }

    syn::parse_str::<syn::Ident>(name).map_err(|e| {
        McpError::handler(format!(
            "Invalid handler name '{}': {} (must be valid Rust identifier)",
            name, e
        ))
    })?;

    if RESERVED_HANDLER_NAMES.contains(&name) {
        return Err(McpError::handler(format!(
            "Handler name '{}' is reserved for system use",
            name
        )));
    }

    Ok(())
}

/// No-op when security feature disabled (empty check only)
#[cfg(not(feature = "security"))]
pub fn validate_handler_name(name: &str) -> ServerResult<()> {
    use crate::McpError;

    if name.is_empty() {
        return Err(McpError::handler(
            "Handler name cannot be empty".to_string(),
        ));
    }

    Ok(())
}

#[cfg(all(test, feature = "security"))]
mod tests {
    use super::*;

    #[test]
    fn test_valid_handler_names() {
        // Standard names
        assert!(validate_handler_name("get_user").is_ok());
        assert!(validate_handler_name("fetch_data").is_ok());
        assert!(validate_handler_name("process_request").is_ok());

        // With numbers
        assert!(validate_handler_name("tool_v2").is_ok());
        assert!(validate_handler_name("handler_123").is_ok());

        // Starting with underscore
        assert!(validate_handler_name("_internal").is_ok());
        assert!(validate_handler_name("_helper_function").is_ok());

        // CamelCase (valid as identifier)
        assert!(validate_handler_name("GetUser").is_ok());
        assert!(validate_handler_name("FetchData").is_ok());
    }

    #[test]
    fn test_reject_empty_name() {
        assert!(validate_handler_name("").is_err());
    }

    #[test]
    fn test_reject_reserved_keywords() {
        // Strict keywords
        assert!(validate_handler_name("async").is_err());
        assert!(validate_handler_name("await").is_err());
        assert!(validate_handler_name("fn").is_err());
        assert!(validate_handler_name("impl").is_err());
        assert!(validate_handler_name("let").is_err());
        assert!(validate_handler_name("match").is_err());
        assert!(validate_handler_name("struct").is_err());
        assert!(validate_handler_name("type").is_err());
    }

    #[test]
    fn test_reject_reserved_system_names() {
        assert!(validate_handler_name("initialize").is_err());
        assert!(validate_handler_name("shutdown").is_err());
        assert!(validate_handler_name("admin").is_err());
        assert!(validate_handler_name("root").is_err());
        assert!(validate_handler_name("system").is_err());
    }

    #[test]
    fn test_reject_path_traversal() {
        assert!(validate_handler_name("../etc/passwd").is_err());
        assert!(validate_handler_name("../../secret").is_err());
        assert!(validate_handler_name("..").is_err());
    }

    #[test]
    fn test_reject_invalid_characters() {
        // Hyphens
        assert!(validate_handler_name("foo-bar").is_err());

        // Spaces
        assert!(validate_handler_name("foo bar").is_err());

        // Special characters
        assert!(validate_handler_name("foo@bar").is_err());
        assert!(validate_handler_name("foo#bar").is_err());
        assert!(validate_handler_name("foo$bar").is_err());

        // Path separators
        assert!(validate_handler_name("foo/bar").is_err());
        assert!(validate_handler_name("foo\\bar").is_err());
    }

    #[test]
    fn test_reject_starting_with_number() {
        assert!(validate_handler_name("1foo").is_err());
        assert!(validate_handler_name("123").is_err());
    }

    #[test]
    fn test_reject_sql_injection_attempts() {
        assert!(validate_handler_name("'; DROP TABLE users; --").is_err());
        assert!(validate_handler_name("foo\"; DELETE FROM handlers; --").is_err());
    }

    #[test]
    fn test_reject_command_injection_attempts() {
        assert!(validate_handler_name("; rm -rf /").is_err());
        assert!(validate_handler_name("| cat /etc/passwd").is_err());
        assert!(validate_handler_name("$(whoami)").is_err());
    }

    #[test]
    fn test_reject_xss_attempts() {
        assert!(validate_handler_name("<script>alert(1)</script>").is_err());
        assert!(validate_handler_name("javascript:alert(1)").is_err());
    }

    #[test]
    fn test_reject_overly_long_names() {
        let long_name = "a".repeat(MAX_HANDLER_NAME_LENGTH + 1);
        assert!(validate_handler_name(&long_name).is_err());
    }

    #[test]
    fn test_accept_max_length_names() {
        let max_name = "a".repeat(MAX_HANDLER_NAME_LENGTH);
        assert!(validate_handler_name(&max_name).is_ok());
    }
}
