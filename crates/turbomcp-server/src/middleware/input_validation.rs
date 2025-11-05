//! Input Validation Middleware using garde + axum-valid (Sprint 3.4)
//!
//! This module provides production-grade input validation using the garde validation library
//! integrated with Axum via axum-valid. It prevents injection attacks, malformed data, and
//! ensures data integrity across the MCP server.
//!
//! ## Security (Sprint 3.4)
//!
//! - Derive-based validation with `#[derive(Validate)]`
//! - 14+ built-in validators (email, URL, length, range, regex, etc.)
//! - Automatic validation extraction in Axum handlers
//! - Type-safe validation errors with 400 Bad Request responses
//! - Latest versions: garde 0.22.0 + axum-valid 0.24.0
//!
//! ## Features
//!
//! - **Email Validation**: RFC 5322 compliant email validation
//! - **URL Validation**: Validates HTTP/HTTPS URLs
//! - **Phone Numbers**: International phone number validation
//! - **Credit Cards**: Luhn algorithm validation
//! - **Length/Range**: String length and numeric range validation
//! - **Pattern Matching**: Regex pattern validation
//! - **Custom Validators**: Domain-specific validation logic
//!
//! ## Usage with Axum
//!
//! ```rust,ignore
//! use axum::{Json, Router, routing::post};
//! use axum_valid::Garde;
//! use garde::Validate;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, Deserialize, Validate)]
//! struct CreateUser {
//!     #[garde(email)]
//!     email: String,
//!
//!     #[garde(length(min = 8, max = 128))]
//!     #[garde(pattern(r"^[a-zA-Z0-9_-]+$"))]
//!     username: String,
//!
//!     #[garde(length(min = 12))]
//!     password: String,
//!
//!     #[garde(range(min = 18, max = 120))]
//!     age: u8,
//! }
//!
//! async fn create_user(Garde(Json(user)): Garde<Json<CreateUser>>) -> Json<String> {
//!     // Input is already validated here
//!     Json(format!("Created user: {}", user.username))
//! }
//!
//! let app = Router::new()
//!     .route("/users", post(create_user));
//! ```
//!
//! ## Available Validators
//!
//! - `#[garde(email)]` - HTML5 email validation
//! - `#[garde(url)]` - URL validation
//! - `#[garde(ip)]`, `#[garde(ipv4)]`, `#[garde(ipv6)]` - IP address validation
//! - `#[garde(credit_card)]` - Credit card validation (Luhn algorithm)
//! - `#[garde(phone_number)]` - Phone number validation
//! - `#[garde(length(min = X, max = Y))]` - Length validation (bytes/chars/graphemes/utf16)
//! - `#[garde(range(min = X, max = Y))]` - Numeric range validation
//! - `#[garde(ascii)]` - ASCII-only validation
//! - `#[garde(alphanumeric)]` - Alphanumeric validation
//! - `#[garde(pattern(regex))]` - Regex pattern matching
//! - `#[garde(contains(substring))]` - Substring validation
//! - `#[garde(prefix(str))`, `#[garde(suffix(str))]` - Prefix/suffix validation
//! - `#[garde(dive)]` - Nested validation for containers
//! - `#[garde(custom(function))]` - Custom validation logic
//!
//! ## Best Practices
//!
//! 1. **Always validate at boundaries**: Validate all external input (HTTP requests, files, etc.)
//! 2. **Use strict rules**: Prefer explicit validation rules over permissive ones
//! 3. **Combine validators**: Stack multiple validators for defense in depth
//! 4. **Custom validators**: Use `#[garde(custom)]` for domain-specific logic
//! 5. **Nested validation**: Use `#[garde(dive)]` for complex nested structures
//!
//! ## Security Properties
//!
//! - **Injection Prevention**: Validates input before processing (prevents SQL injection, XSS, etc.)
//! - **DoS Prevention**: Length/range validators prevent resource exhaustion
//! - **Type Safety**: Compile-time validation rule checking
//! - **Zero Allocations**: Validation happens in-place when possible
//! - **Error Context**: Detailed error messages for debugging (sanitized in production)

#[cfg(feature = "input-validation")]
#[allow(unused_imports)] // Re-exported for public API
pub use garde::Validate;

#[cfg(feature = "input-validation")]
pub use axum_valid::Garde;

/// Common validation patterns
#[cfg(feature = "input-validation")]
pub mod patterns {
    /// Username pattern: alphanumeric, underscore, hyphen (3-32 chars)
    pub const USERNAME: &str = r"^[a-zA-Z0-9_-]{3,32}$";

    /// Slug pattern: lowercase, numbers, hyphen (1-64 chars)
    pub const SLUG: &str = r"^[a-z0-9-]{1,64}$";

    /// UUID v4 pattern
    pub const UUID_V4: &str =
        r"^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$";

    /// Hex color pattern
    pub const HEX_COLOR: &str = r"^#[0-9a-fA-F]{6}$";

    /// Semver pattern (simplified)
    pub const SEMVER: &str = r"^\d+\.\d+\.\d+$";

    /// ISO 8601 date pattern (YYYY-MM-DD)
    pub const ISO_DATE: &str = r"^\d{4}-\d{2}-\d{2}$";
}

/// Common reusable validator functions
#[cfg(feature = "input-validation")]
pub mod validators {
    /// Validate that a string is a valid MCP method name
    ///
    /// MCP method names follow the pattern: `category/action` or `category/subcategory/action`
    ///
    /// Examples:
    /// - `tools/list`
    /// - `tools/call`
    /// - `prompts/get`
    /// - `resources/read`
    pub fn mcp_method_name(value: &str, _context: &()) -> garde::Result {
        if value.is_empty() {
            return Err(garde::Error::new("MCP method name cannot be empty"));
        }

        if value.len() > 128 {
            return Err(garde::Error::new(
                "MCP method name too long (max 128 chars)",
            ));
        }

        // Check for valid characters (alphanumeric, forward slash, underscore, hyphen)
        if !value
            .chars()
            .all(|c| c.is_alphanumeric() || c == '/' || c == '_' || c == '-')
        {
            return Err(garde::Error::new(
                "MCP method name can only contain alphanumeric, '/', '_', '-'",
            ));
        }

        // Validate structure: at least one slash, no double slashes, no leading/trailing slashes
        if !value.contains('/') {
            return Err(garde::Error::new("MCP method name must contain '/'"));
        }

        if value.contains("//") {
            return Err(garde::Error::new("MCP method name cannot contain '//'"));
        }

        if value.starts_with('/') || value.ends_with('/') {
            return Err(garde::Error::new(
                "MCP method name cannot start or end with '/'",
            ));
        }

        Ok(())
    }

    /// Validate that a string is a safe file path (no path traversal)
    pub fn safe_file_path(value: &str, _context: &()) -> garde::Result {
        if value.is_empty() {
            return Err(garde::Error::new("File path cannot be empty"));
        }

        // Prevent path traversal
        if value.contains("..") {
            return Err(garde::Error::new("File path cannot contain '..'"));
        }

        // Prevent absolute paths
        if value.starts_with('/') || value.contains(':') {
            return Err(garde::Error::new("File path must be relative"));
        }

        // Prevent null bytes
        if value.contains('\0') {
            return Err(garde::Error::new("File path cannot contain null bytes"));
        }

        Ok(())
    }

    /// Validate that a string is a safe identifier (alphanumeric + underscore)
    pub fn safe_identifier(value: &str, _context: &()) -> garde::Result {
        if value.is_empty() {
            return Err(garde::Error::new("Identifier cannot be empty"));
        }

        if value.len() > 128 {
            return Err(garde::Error::new("Identifier too long (max 128 chars)"));
        }

        // Must start with letter
        if !value.chars().next().unwrap().is_alphabetic() {
            return Err(garde::Error::new("Identifier must start with a letter"));
        }

        // Must be alphanumeric + underscore
        if !value.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(garde::Error::new(
                "Identifier can only contain alphanumeric and '_'",
            ));
        }

        Ok(())
    }
}

#[cfg(all(test, feature = "input-validation"))]
mod tests {
    use super::validators;

    // Note: Tests for garde derive macro would be in user code.
    // Here we test the custom validators we provide.

    #[test]
    fn test_mcp_method_name_valid() {
        assert!(validators::mcp_method_name("tools/list", &()).is_ok());
        assert!(validators::mcp_method_name("tools/call", &()).is_ok());
        assert!(validators::mcp_method_name("prompts/get", &()).is_ok());
        assert!(validators::mcp_method_name("resources/templates/list", &()).is_ok());
    }

    #[test]
    fn test_mcp_method_name_invalid() {
        // No slash
        assert!(validators::mcp_method_name("toolslist", &()).is_err());

        // Double slash
        assert!(validators::mcp_method_name("tools//list", &()).is_err());

        // Leading slash
        assert!(validators::mcp_method_name("/tools/list", &()).is_err());

        // Trailing slash
        assert!(validators::mcp_method_name("tools/list/", &()).is_err());

        // Invalid characters
        assert!(validators::mcp_method_name("tools@list", &()).is_err());
    }

    #[test]
    fn test_safe_file_path_valid() {
        assert!(validators::safe_file_path("data/file.txt", &()).is_ok());
        assert!(validators::safe_file_path("images/logo.png", &()).is_ok());
    }

    #[test]
    fn test_safe_file_path_traversal() {
        // Path traversal
        assert!(validators::safe_file_path("../etc/passwd", &()).is_err());
        assert!(validators::safe_file_path("data/../../../etc/passwd", &()).is_err());

        // Absolute path
        assert!(validators::safe_file_path("/etc/passwd", &()).is_err());

        // Null byte
        assert!(validators::safe_file_path("file\0.txt", &()).is_err());
    }

    #[test]
    fn test_safe_identifier_valid() {
        assert!(validators::safe_identifier("user_id", &()).is_ok());
        assert!(validators::safe_identifier("firstName", &()).is_ok());
        assert!(validators::safe_identifier("CONSTANT_VALUE", &()).is_ok());
    }

    #[test]
    fn test_safe_identifier_invalid() {
        // Starts with number
        assert!(validators::safe_identifier("1user", &()).is_err());

        // Contains invalid characters
        assert!(validators::safe_identifier("user-id", &()).is_err());
        assert!(validators::safe_identifier("user@id", &()).is_err());

        // Empty
        assert!(validators::safe_identifier("", &()).is_err());
    }
}
