//! Security utilities for error sanitization and input validation.
//!
//! This module provides `no_std` compatible security primitives for:
//! - Error message sanitization (OWASP compliant)
//! - Input size validation
//! - URI scheme validation
//!
//! ## Error Sanitization
//!
//! Production systems should never expose internal details in error messages.
//! Use [`sanitize_error_message`] to redact sensitive information:
//!
//! ```rust
//! use turbomcp_core::security::sanitize_error_message;
//!
//! let unsafe_msg = "Failed to connect to postgres://admin:secret@192.168.1.100:5432/db";
//! let safe_msg = sanitize_error_message(unsafe_msg);
//! assert!(!safe_msg.contains("secret"));
//! assert!(!safe_msg.contains("192.168.1.100"));
//! ```
//!
//! ## Input Validation
//!
//! Use [`InputLimits`] to validate parameter sizes:
//!
//! ```rust
//! use turbomcp_core::security::InputLimits;
//!
//! let limits = InputLimits::default();
//! assert!(limits.check_string_length("short").is_ok());
//! ```

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// Maximum parameter string length (1MB by default)
pub const DEFAULT_MAX_STRING_LENGTH: usize = 1024 * 1024;

/// Maximum parameter name length
pub const DEFAULT_MAX_PARAM_NAME_LENGTH: usize = 256;

/// Maximum URI length (8KB - standard browser limit)
pub const DEFAULT_MAX_URI_LENGTH: usize = 8192;

/// Maximum number of parameters per tool call
pub const DEFAULT_MAX_PARAMS: usize = 100;

/// Configuration for input validation limits.
///
/// Use this to enforce size constraints on user input to prevent DoS attacks.
#[derive(Debug, Clone, Copy)]
pub struct InputLimits {
    /// Maximum string parameter length in bytes
    pub max_string_length: usize,
    /// Maximum parameter name length
    pub max_param_name_length: usize,
    /// Maximum URI length
    pub max_uri_length: usize,
    /// Maximum number of parameters
    pub max_params: usize,
}

impl Default for InputLimits {
    fn default() -> Self {
        Self {
            max_string_length: DEFAULT_MAX_STRING_LENGTH,
            max_param_name_length: DEFAULT_MAX_PARAM_NAME_LENGTH,
            max_uri_length: DEFAULT_MAX_URI_LENGTH,
            max_params: DEFAULT_MAX_PARAMS,
        }
    }
}

impl InputLimits {
    /// Create limits for production use (stricter)
    #[must_use]
    pub const fn production() -> Self {
        Self {
            max_string_length: 64 * 1024, // 64KB
            max_param_name_length: 128,
            max_uri_length: 2048,
            max_params: 50,
        }
    }

    /// Create limits for development (more permissive)
    #[must_use]
    pub const fn development() -> Self {
        Self {
            max_string_length: 10 * 1024 * 1024, // 10MB
            max_param_name_length: 512,
            max_uri_length: 65536,
            max_params: 1000,
        }
    }

    /// Check if a string parameter is within limits
    pub fn check_string_length(&self, s: &str) -> Result<(), InputValidationError> {
        if s.len() > self.max_string_length {
            return Err(InputValidationError::StringTooLong {
                actual: s.len(),
                max: self.max_string_length,
            });
        }
        Ok(())
    }

    /// Check if a parameter name is within limits
    pub fn check_param_name(&self, name: &str) -> Result<(), InputValidationError> {
        if name.len() > self.max_param_name_length {
            return Err(InputValidationError::ParamNameTooLong {
                actual: name.len(),
                max: self.max_param_name_length,
            });
        }
        Ok(())
    }

    /// Check if a URI is within limits
    pub fn check_uri_length(&self, uri: &str) -> Result<(), InputValidationError> {
        if uri.len() > self.max_uri_length {
            return Err(InputValidationError::UriTooLong {
                actual: uri.len(),
                max: self.max_uri_length,
            });
        }
        Ok(())
    }

    /// Check if parameter count is within limits
    pub fn check_param_count(&self, count: usize) -> Result<(), InputValidationError> {
        if count > self.max_params {
            return Err(InputValidationError::TooManyParams {
                actual: count,
                max: self.max_params,
            });
        }
        Ok(())
    }
}

/// Input validation errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputValidationError {
    /// String parameter exceeds maximum length
    StringTooLong {
        /// Actual length
        actual: usize,
        /// Maximum allowed
        max: usize,
    },
    /// Parameter name exceeds maximum length
    ParamNameTooLong {
        /// Actual length
        actual: usize,
        /// Maximum allowed
        max: usize,
    },
    /// URI exceeds maximum length
    UriTooLong {
        /// Actual length
        actual: usize,
        /// Maximum allowed
        max: usize,
    },
    /// Too many parameters
    TooManyParams {
        /// Actual count
        actual: usize,
        /// Maximum allowed
        max: usize,
    },
    /// Invalid URI scheme
    InvalidUriScheme {
        /// The scheme that was rejected
        scheme: String,
    },
}

impl core::fmt::Display for InputValidationError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::StringTooLong { actual, max } => {
                write!(f, "String too long: {} bytes (max: {})", actual, max)
            }
            Self::ParamNameTooLong { actual, max } => {
                write!(
                    f,
                    "Parameter name too long: {} bytes (max: {})",
                    actual, max
                )
            }
            Self::UriTooLong { actual, max } => {
                write!(f, "URI too long: {} bytes (max: {})", actual, max)
            }
            Self::TooManyParams { actual, max } => {
                write!(f, "Too many parameters: {} (max: {})", actual, max)
            }
            Self::InvalidUriScheme { scheme } => {
                write!(f, "Invalid URI scheme: {}", scheme)
            }
        }
    }
}

/// Allowed URI schemes for resource access.
///
/// Only these schemes are permitted by default:
/// - `file` - Local file access
/// - `http` / `https` - Web resources
/// - `data` - Data URIs
/// - `mcp` - MCP-specific resources
pub const ALLOWED_URI_SCHEMES: &[&str] = &["file", "http", "https", "data", "mcp"];

/// Validate a URI scheme against the allowlist.
///
/// Returns the scheme if valid, or an error if not allowed.
/// Handles both standard URIs (scheme://...) and data URIs (data:...).
pub fn validate_uri_scheme(uri: &str) -> Result<&str, InputValidationError> {
    // Extract scheme - handle both "scheme://..." and "scheme:..." (for data URIs)
    let scheme = if let Some(pos) = uri.find("://") {
        &uri[..pos]
    } else if let Some(pos) = uri.find(':') {
        &uri[..pos]
    } else {
        ""
    };

    if scheme.is_empty() || !ALLOWED_URI_SCHEMES.contains(&scheme) {
        return Err(InputValidationError::InvalidUriScheme {
            scheme: String::from(scheme),
        });
    }

    Ok(scheme)
}

/// Sanitize an error message by redacting sensitive information.
///
/// This function removes/replaces:
/// - File paths (Unix and Windows)
/// - IP addresses (IPv4)
/// - Connection strings (database URLs)
/// - Secrets (API keys, tokens, passwords)
/// - Email addresses
/// - URLs with credentials
///
/// This is a `no_std` compatible implementation without regex.
///
/// # Example
///
/// ```rust
/// use turbomcp_core::security::sanitize_error_message;
///
/// let msg = "Connection failed to postgres://user:pass@localhost:5432/db";
/// let safe = sanitize_error_message(msg);
/// assert!(!safe.contains("pass"));
/// ```
#[must_use]
pub fn sanitize_error_message(message: &str) -> String {
    let mut result = String::from(message);

    // Sanitize in order of specificity (most specific patterns first)
    result = sanitize_connection_strings(&result);
    result = sanitize_urls_with_credentials(&result);
    result = sanitize_secrets(&result);
    result = sanitize_ip_addresses(&result);
    result = sanitize_file_paths(&result);
    result = sanitize_emails(&result);

    result
}

/// Sanitize connection strings (database URLs)
fn sanitize_connection_strings(s: &str) -> String {
    let prefixes = [
        "postgres://",
        "postgresql://",
        "mysql://",
        "mongodb://",
        "redis://",
        "amqp://",
        "kafka://",
        "sqlite://",
    ];

    let mut result = String::from(s);
    for prefix in prefixes {
        while let Some(start) = result.find(prefix) {
            let end = result[start..]
                .find(|c: char| c.is_whitespace() || c == '\'' || c == '"')
                .map(|i| start + i)
                .unwrap_or(result.len());
            result.replace_range(start..end, "[CONNECTION]");
        }
    }
    result
}

/// Sanitize URLs containing credentials (user:pass@host)
fn sanitize_urls_with_credentials(s: &str) -> String {
    let mut result = String::from(s);

    for prefix in ["http://", "https://", "ftp://"] {
        while let Some(start) = result.find(prefix) {
            let after_proto = start + prefix.len();
            let rest = &result[after_proto..];

            // Check if this URL has credentials (contains @ before /)
            if let Some(at_pos) = rest.find('@') {
                let slash_pos = rest.find('/').unwrap_or(rest.len());
                if at_pos < slash_pos {
                    // This URL has credentials - redact them
                    let end = result[start..]
                        .find(|c: char| c.is_whitespace() || c == '\'' || c == '"')
                        .map(|i| start + i)
                        .unwrap_or(result.len());
                    result.replace_range(start..end, "[URL]");
                    continue;
                }
            }

            // No credentials, skip this occurrence
            break;
        }
    }
    result
}

/// Sanitize secret patterns (api_key=..., password=..., token=..., etc.)
fn sanitize_secrets(s: &str) -> String {
    let patterns = [
        "api_key=",
        "api-key=",
        "apikey=",
        "password=",
        "passwd=",
        "token=",
        "secret=",
        "api_key:",
        "api-key:",
        "password:",
        "token:",
        "secret:",
        "Bearer ",
        "bearer ",
        "Authorization: ",
    ];

    let mut result = String::from(s);
    let lower = s.to_lowercase();

    // Process each pattern once (find all occurrences, then replace from back to front)
    for pattern in patterns {
        let pattern_lower = pattern.to_lowercase();
        let mut positions: Vec<usize> = Vec::new();

        let mut search_start = 0;
        while let Some(pos) = lower[search_start..].find(&pattern_lower) {
            positions.push(search_start + pos);
            search_start += pos + pattern.len();
        }

        // Replace from back to front to preserve earlier positions
        for start in positions.into_iter().rev() {
            let prefix_end = start + pattern.len();
            if prefix_end >= result.len() {
                continue;
            }

            // Find end of secret value (whitespace, comma, semicolon, quote, or end)
            let end = result[prefix_end..]
                .find(|c: char| {
                    c.is_whitespace() || c == ',' || c == ';' || c == '\'' || c == '"' || c == ')'
                })
                .map(|i| prefix_end + i)
                .unwrap_or(result.len());

            // Build replacement string (preserve original case of keyword)
            let keyword = &result[start..start + pattern.len()];
            let replacement = if keyword.ends_with('=') {
                format!("{}=[REDACTED]", keyword.trim_end_matches('='))
            } else if keyword.ends_with(':') {
                format!("{}:[REDACTED]", keyword.trim_end_matches(':'))
            } else {
                format!("{} [REDACTED]", keyword.trim())
            };

            result.replace_range(start..end, &replacement);
        }
    }
    result
}

/// Sanitize IPv4 addresses
fn sanitize_ip_addresses(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        // Check if this might be start of an IP address
        if c.is_ascii_digit() {
            let mut potential_ip = String::from(c);
            let mut dot_count = 0;
            let mut is_ip = true;
            let mut segment_digits = 1;

            // Try to collect an IP address
            while let Some(&next) = chars.peek() {
                if next.is_ascii_digit() {
                    segment_digits += 1;
                    if segment_digits > 3 {
                        is_ip = false;
                        break;
                    }
                    potential_ip.push(chars.next().unwrap());
                } else if next == '.' && dot_count < 3 {
                    dot_count += 1;
                    segment_digits = 0;
                    potential_ip.push(chars.next().unwrap());
                } else {
                    break;
                }
            }

            // Validate: must have exactly 3 dots and valid segments
            if is_ip && dot_count == 3 {
                let segments: Vec<&str> = potential_ip.split('.').collect();
                let valid_ip = segments.len() == 4
                    && segments
                        .iter()
                        .all(|seg| !seg.is_empty() && seg.len() <= 3 && seg.parse::<u8>().is_ok());

                if valid_ip {
                    result.push_str("[IP]");
                } else {
                    result.push_str(&potential_ip);
                }
            } else {
                result.push_str(&potential_ip);
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Sanitize file paths (Unix and Windows)
fn sanitize_file_paths(s: &str) -> String {
    let mut result = String::from(s);

    // Unix absolute paths: /path/to/file
    let mut i = 0;
    while i < result.len() {
        let bytes = result.as_bytes();
        if bytes[i] == b'/' && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric()) {
            // Check if this looks like a path (has more path chars after /)
            let rest = &result[i..];
            if rest.len() > 1
                && (rest
                    .chars()
                    .nth(1)
                    .is_some_and(|c| c.is_alphanumeric() || c == '.'))
            {
                // Find end of path
                let end = rest[1..]
                    .find(|c: char| {
                        c.is_whitespace() || c == '\'' || c == '"' || c == ')' || c == ']'
                    })
                    .map(|p| i + 1 + p)
                    .unwrap_or(result.len());

                // Only redact if it looks like a real path (has / or extension)
                let path_segment = &result[i..end];
                if path_segment.contains('/')
                    || (path_segment.contains('.') && path_segment.len() > 4)
                {
                    result.replace_range(i..end, "[PATH]");
                } else {
                    i += 1;
                }
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    // Windows paths: C:\path\to\file
    let mut i = 0;
    while i < result.len() {
        let bytes = result.as_bytes();
        if i + 2 < bytes.len()
            && bytes[i].is_ascii_alphabetic()
            && bytes[i + 1] == b':'
            && bytes[i + 2] == b'\\'
        {
            // Find end of Windows path
            let end = result[i..]
                .find(|c: char| c.is_whitespace() || c == '\'' || c == '"')
                .map(|p| i + p)
                .unwrap_or(result.len());
            result.replace_range(i..end, "[PATH]");
        }
        i += 1;
    }

    result
}

/// Sanitize email addresses
fn sanitize_emails(s: &str) -> String {
    let mut result = String::from(s);

    // Simple email detection: look for @ followed by domain
    let mut i = 0;
    while i < result.len() {
        if let Some(at_pos) = result[i..].find('@') {
            let abs_at = i + at_pos;

            // Find start of email (username part)
            let start = result[..abs_at]
                .rfind(|c: char| c.is_whitespace() || c == '<' || c == '(' || c == ',')
                .map(|p| p + 1)
                .unwrap_or(0);

            // Validate we have a username
            if start >= abs_at {
                i = abs_at + 1;
                continue;
            }

            // Find end of email (domain part)
            let after_at = abs_at + 1;
            if after_at >= result.len() {
                break;
            }

            // Find end of domain
            let end = result[after_at..]
                .find(|c: char| c.is_whitespace() || c == '>' || c == ')' || c == ',')
                .map(|p| after_at + p)
                .unwrap_or(result.len());

            // Validate domain has a dot
            let domain = &result[after_at..end];
            if domain.contains('.') && domain.len() > 3 {
                result.replace_range(start..end, "[EMAIL]");
                i = start + 7; // length of "[EMAIL]"
            } else {
                i = end;
            }
        } else {
            break;
        }
    }

    result
}

/// Generic safe error message for production.
pub const GENERIC_ERROR_MESSAGE: &str = "An error occurred. Please try again.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_connection_strings() {
        let msg = "Failed: postgres://admin:secret@localhost:5432/mydb";
        let safe = sanitize_error_message(msg);
        assert!(!safe.contains("admin"));
        assert!(!safe.contains("secret"));
        assert!(safe.contains("[CONNECTION]"));
    }

    #[test]
    fn test_sanitize_ip_addresses() {
        let msg = "Connection to 192.168.1.100:5432 failed";
        let safe = sanitize_error_message(msg);
        assert!(!safe.contains("192.168.1.100"));
        assert!(safe.contains("[IP]"));
    }

    #[test]
    fn test_sanitize_file_paths() {
        let msg = "File not found: /etc/secrets/api_key.txt";
        let safe = sanitize_error_message(msg);
        assert!(!safe.contains("/etc/secrets"));
        assert!(safe.contains("[PATH]"));
    }

    #[test]
    fn test_sanitize_secrets() {
        let msg = "Auth failed: api_key=sk_live_abc123xyz";
        let safe = sanitize_error_message(msg);
        assert!(!safe.contains("sk_live"));
        assert!(safe.contains("[REDACTED]"));
    }

    #[test]
    fn test_sanitize_emails() {
        let msg = "User admin@example.com not found";
        let safe = sanitize_error_message(msg);
        assert!(!safe.contains("admin@example.com"));
        assert!(safe.contains("[EMAIL]"));
    }

    #[test]
    fn test_input_limits() {
        let limits = InputLimits::production();
        assert!(limits.check_string_length("short").is_ok());

        let long_string = "x".repeat(limits.max_string_length + 1);
        assert!(limits.check_string_length(&long_string).is_err());
    }

    #[test]
    fn test_uri_scheme_validation() {
        assert!(validate_uri_scheme("file:///etc/passwd").is_ok());
        assert!(validate_uri_scheme("https://example.com").is_ok());
        assert!(validate_uri_scheme("javascript:alert(1)").is_err());
        assert!(validate_uri_scheme("data:text/html,hello").is_ok());
    }

    #[test]
    fn test_no_false_positives() {
        let msg = "User 123 requested tool list on port 8080";
        let safe = sanitize_error_message(msg);
        // Should not sanitize normal numbers or port references
        assert_eq!(msg, safe);
    }
}
