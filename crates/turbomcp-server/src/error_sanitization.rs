//! Error Message Sanitization (Sprint 3.1)
//!
//! Prevents information leakage in error messages according to OWASP best practices.
//!
//! ## Security Risks (OWASP)
//!
//! Error messages can leak sensitive information to attackers:
//! - **File paths**: `"/Users/admin/project/src/main.rs"` → `"[PATH]"`
//! - **IP addresses**: `"192.168.1.100"` → `"[IP]"`
//! - **Connection strings**: `"postgres://user:pass@host/db"` → `"[CONNECTION]"`
//! - **Stack traces**: Full traces → Generic "An error occurred"
//! - **System information**: Versions, environment details
//!
//! ## Display Modes
//!
//! - **Production**: Sanitizes all sensitive information, generic messages
//! - **Development**: Shows full details for debugging
//!
//! ## Usage
//!
//! ```rust,ignore
//! use turbomcp_server::error_sanitization::{SanitizedError, DisplayMode};
//!
//! let error = std::io::Error::new(
//!     std::io::ErrorKind::NotFound,
//!     "File not found: /etc/secrets/api_key.txt"
//! );
//!
//! // Production: Redacts file path
//! let sanitized = SanitizedError::new(error, DisplayMode::Production);
//! println!("{}", sanitized); // "File not found: [PATH]"
//!
//! // Development: Shows full details
//! let detailed = SanitizedError::new(error, DisplayMode::Development);
//! println!("{}", detailed); // "File not found: /etc/secrets/api_key.txt"
//! ```

use regex::Regex;
use std::sync::OnceLock;

/// Display mode for error messages according to OWASP guidelines.
///
/// Controls whether error messages are sanitized to prevent information leakage.
/// Default is Production for safety.
///
/// # Security Considerations
///
/// Error messages can leak sensitive information including:
/// - File system paths (e.g., `/etc/secrets/key.txt`)
/// - IP addresses and network information (e.g., `192.168.1.100`)
/// - Database connection strings (e.g., `postgres://user:pass@host/db`)
/// - API keys and authentication tokens
/// - Email addresses and other PII
/// - Stack traces and implementation details
///
/// Use Production mode in all customer-facing systems to prevent information disclosure attacks.
/// Only use Development mode when debugging in secure environments.
///
/// # Examples
///
/// ```ignore
/// use turbomcp_server::error_sanitization::{SanitizedError, DisplayMode};
///
/// let error = std::io::Error::new(
///     std::io::ErrorKind::NotFound,
///     "File /etc/secrets/password.txt not found"
/// );
///
/// // Production mode: Redacts sensitive paths
/// let prod_error = SanitizedError::new(&error, DisplayMode::Production);
/// println!("{}", prod_error);  // "File [PATH] not found"
///
/// // Development mode: Shows full details for debugging
/// let dev_error = SanitizedError::new(&error, DisplayMode::Development);
/// println!("{}", dev_error);   // "File /etc/secrets/password.txt not found"
/// ```
///
/// # OWASP Reference
/// See OWASP Top 10: A01:2021 – Broken Access Control for information disclosure risks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisplayMode {
    /// Production mode: Sanitize all sensitive information (default for safety).
    ///
    /// Recommended for all customer-facing systems, APIs, and logs that might
    /// be accessed by untrusted parties. Redacts:
    /// - File paths → `[PATH]`
    /// - IP addresses → `[IP]`
    /// - Connection strings → `[CONNECTION]`
    /// - API keys and tokens → `[REDACTED]`
    /// - Email addresses → `[EMAIL]`
    /// - URLs → `[URL]`
    #[default]
    Production,
    /// Development mode: Show full error details without sanitization.
    ///
    /// Only use in development, testing, and secure environments where
    /// debugging information leakage is acceptable. Shows full error messages
    /// including paths, IPs, and other sensitive details.
    Development,
}

/// Wrapper that sanitizes error messages according to security requirements.
///
/// This generic wrapper can wrap any error type and sanitize its display output.
/// It implements `Display` and `Error` traits, making it compatible with Rust's
/// standard error handling.
///
/// # Purpose
///
/// Prevents accidental information leakage from error messages in production environments
/// while preserving full error details in development. This is critical for preventing
/// attackers from using error messages to:
/// - Map the internal system architecture
/// - Identify file paths and system configuration
/// - Discover IP addresses and network topology
/// - Extract API keys, passwords, and other secrets
///
/// # Usage
///
/// ```ignore
/// use turbomcp_server::error_sanitization::{SanitizedError, DisplayMode};
///
/// // Wrap an existing error for sanitization
/// let io_error = std::io::Error::new(
///     std::io::ErrorKind::PermissionDenied,
///     "Access denied to /etc/shadow"
/// );
///
/// // Automatically redact sensitive information when displayed
/// let sanitized = SanitizedError::production(io_error);
/// println!("{}", sanitized);  // "Access denied to [PATH]"
///
/// // Extract the original error if needed
/// let original = sanitized.into_inner();
/// ```
///
/// # Thread Safety
///
/// This wrapper is `Send + Sync` if the wrapped error is. Safe to use in concurrent contexts.
///
/// # Performance
///
/// The sanitization happens lazily during `Display` implementation. The error is not
/// modified until it's converted to a string, so there's minimal overhead for errors
/// that are never displayed.
#[derive(Debug)]
pub struct SanitizedError<E> {
    error: E,
    mode: DisplayMode,
}

impl<E> SanitizedError<E> {
    /// Create a new sanitized error with specified display mode.
    ///
    /// # Arguments
    ///
    /// * `error` - The underlying error to wrap
    /// * `mode` - Whether to sanitize (Production) or show full details (Development)
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let error = std::io::Error::last_os_error();
    /// let sanitized = SanitizedError::new(error, DisplayMode::Production);
    /// ```
    pub fn new(error: E, mode: DisplayMode) -> Self {
        Self { error, mode }
    }

    /// Create a production-mode sanitized error (recommended for most cases).
    ///
    /// This is the recommended constructor for error wrappers that will be
    /// displayed to users or in logs that might be accessed by untrusted parties.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let error = std::fs::read("private.txt")
    ///     .map_err(SanitizedError::production)?;
    /// ```
    pub fn production(error: E) -> Self {
        Self::new(error, DisplayMode::Production)
    }

    /// Create a development-mode sanitized error (shows full error details).
    ///
    /// Only use this in development and testing environments. In production,
    /// development-mode errors can leak sensitive information.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let error = std::fs::read("config.yml")
    ///     .map_err(SanitizedError::development)?;  // Shows full path
    /// ```
    pub fn development(error: E) -> Self {
        Self::new(error, DisplayMode::Development)
    }

    /// Consume the wrapper and extract the original error.
    ///
    /// Useful if you need access to the underlying error type after wrapping.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let sanitized = SanitizedError::production(some_error);
    /// let original = sanitized.into_inner();
    /// // Now you have the original error back
    /// ```
    pub fn into_inner(self) -> E {
        self.error
    }

    /// Get a reference to the wrapped error.
    ///
    /// Allows reading the original error without taking ownership.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let sanitized = SanitizedError::production(some_error);
    /// let original_ref = sanitized.inner();
    /// ```
    pub fn inner(&self) -> &E {
        &self.error
    }
}

impl<E: std::fmt::Display> std::fmt::Display for SanitizedError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.mode {
            DisplayMode::Development => write!(f, "{}", self.error),
            DisplayMode::Production => {
                let message = self.error.to_string();
                write!(f, "{}", sanitize_error_message(&message))
            }
        }
    }
}

impl<E: std::error::Error> std::error::Error for SanitizedError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.error.source()
    }
}

/// Sanitize an error message by redacting sensitive information
///
/// # What is Sanitized
///
/// - **File paths**: Unix and Windows paths → `[PATH]`
/// - **IP addresses**: IPv4 and IPv6 → `[IP]`
/// - **Connection strings**: Database URLs, etc. → `[CONNECTION]`
/// - **Secrets**: API keys, tokens → `[REDACTED]`
/// - **Email addresses**: Personal emails → `[EMAIL]`
/// - **URLs**: Full URLs → `[URL]`
///
/// # Examples
///
/// ```
/// use turbomcp_server::error_sanitization::sanitize_error_message;
///
/// // Note: Paths with "secret" keyword will trigger secret sanitization
/// assert_eq!(
///     sanitize_error_message("File not found: /etc/config/app.txt"),
///     "File not found: [PATH]"
/// );
///
/// assert_eq!(
///     sanitize_error_message("Connection failed to 192.168.1.100:5432"),
///     "Connection failed to [IP]:5432"
/// );
/// ```
pub fn sanitize_error_message(message: &str) -> String {
    let mut sanitized = message.to_string();

    // IMPORTANT: Order matters! Connection strings and URLs must be sanitized
    // BEFORE IP addresses and file paths, otherwise they get broken up.

    // 1. Sanitize connection strings (database URLs, etc.) - FIRST!
    sanitized = sanitize_connection_strings(&sanitized);

    // 2. Sanitize URLs - SECOND (before IP addresses)
    sanitized = sanitize_urls(&sanitized);

    // 3. Sanitize secrets (API keys, tokens, etc.)
    sanitized = sanitize_secrets(&sanitized);

    // 4. Sanitize IP addresses (IPv4 and IPv6)
    sanitized = sanitize_ip_addresses(&sanitized);

    // 5. Sanitize file paths (both Unix and Windows)
    sanitized = sanitize_file_paths(&sanitized);

    // 6. Sanitize email addresses
    sanitized = sanitize_email_addresses(&sanitized);

    sanitized
}

/// Sanitize Unix and Windows file paths
fn sanitize_file_paths(message: &str) -> String {
    static UNIX_PATH_RE: OnceLock<Regex> = OnceLock::new();
    static WINDOWS_PATH_RE: OnceLock<Regex> = OnceLock::new();

    // Unix paths: /path/to/file or ./relative/path
    let unix_re = UNIX_PATH_RE.get_or_init(|| Regex::new(r"(?:/|\./)[\w\-./]+(?:\.\w+)?").unwrap());

    // Windows paths: C:\path\to\file or \\network\share
    let windows_re = WINDOWS_PATH_RE
        .get_or_init(|| Regex::new(r"(?:[A-Za-z]:\\|\\\\)[\w\-\\/.]+(?:\.\w+)?").unwrap());

    let mut sanitized = unix_re.replace_all(message, "[PATH]").to_string();
    sanitized = windows_re.replace_all(&sanitized, "[PATH]").to_string();

    sanitized
}

/// Sanitize IPv4 and IPv6 addresses
fn sanitize_ip_addresses(message: &str) -> String {
    static IPV4_RE: OnceLock<Regex> = OnceLock::new();
    static IPV6_RE: OnceLock<Regex> = OnceLock::new();

    // IPv4: 192.168.1.1
    let ipv4_re = IPV4_RE.get_or_init(|| Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}\b").unwrap());

    // IPv6: 2001:0db8:85a3:0000:0000:8a2e:0370:7334
    let ipv6_re = IPV6_RE
        .get_or_init(|| Regex::new(r"\b(?:[0-9a-fA-F]{1,4}:){7}[0-9a-fA-F]{1,4}\b").unwrap());

    let mut sanitized = ipv4_re.replace_all(message, "[IP]").to_string();
    sanitized = ipv6_re.replace_all(&sanitized, "[IP]").to_string();

    sanitized
}

/// Sanitize connection strings (database URLs, etc.)
fn sanitize_connection_strings(message: &str) -> String {
    static CONN_STRING_RE: OnceLock<Regex> = OnceLock::new();

    // Match: postgres://user:pass@host:port/db, mysql://..., mongodb://...
    let conn_re = CONN_STRING_RE.get_or_init(|| {
        Regex::new(r"\b(?:postgres|mysql|mongodb|redis|amqp|kafka)://[^\s]+").unwrap()
    });

    conn_re.replace_all(message, "[CONNECTION]").to_string()
}

/// Sanitize secrets (API keys, tokens, passwords)
fn sanitize_secrets(message: &str) -> String {
    static SECRET_RE: OnceLock<Regex> = OnceLock::new();

    // Match: api_key=..., token=..., password=..., secret=..., bearer ...
    // Note: "key" alone is too generic and causes false positives (e.g., "API key:")
    // Captures: (key_name)(separator)(value)
    // Separator can be "=" or ":" or just whitespace (for Bearer tokens)
    let secret_re = SECRET_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(api[_-]?key|token|password|secret|bearer)(\s*[=:]?\s*)([^\s,;)]+)")
            .unwrap()
    });

    // Normalize output: lowercase keyword + "=" separator for consistency
    secret_re
        .replace_all(message, |caps: &regex::Captures| {
            format!("{}=[REDACTED]", caps[1].to_lowercase())
        })
        .to_string()
}

/// Sanitize email addresses
fn sanitize_email_addresses(message: &str) -> String {
    static EMAIL_RE: OnceLock<Regex> = OnceLock::new();

    // Match: user@example.com
    let email_re = EMAIL_RE.get_or_init(|| {
        Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Z|a-z]{2,}\b").unwrap()
    });

    email_re.replace_all(message, "[EMAIL]").to_string()
}

/// Sanitize URLs (HTTP/HTTPS)
fn sanitize_urls(message: &str) -> String {
    static URL_RE: OnceLock<Regex> = OnceLock::new();

    // Match: http://... or https://...
    let url_re = URL_RE.get_or_init(|| Regex::new(r"\b(?:https?|ftp)://[^\s]+").unwrap());

    url_re.replace_all(message, "[URL]").to_string()
}

/// Generic error message for production (OWASP recommendation)
///
/// Use this when you want to completely hide error details from users.
pub const GENERIC_ERROR_MESSAGE: &str = "An error occurred. Please try again or contact support.";

/// Create a generic error response for production
pub fn generic_error() -> String {
    GENERIC_ERROR_MESSAGE.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_unix_paths() {
        assert_eq!(
            sanitize_file_paths("File not found: /etc/secrets/key.txt"),
            "File not found: [PATH]"
        );
        assert_eq!(
            sanitize_file_paths("Error reading ./config/database.yml"),
            "Error reading [PATH]"
        );
        assert_eq!(
            sanitize_file_paths("Failed: /home/user/.ssh/id_rsa"),
            "Failed: [PATH]"
        );
    }

    #[test]
    fn test_sanitize_windows_paths() {
        assert_eq!(
            sanitize_file_paths("File not found: C:\\Windows\\System32\\config.sys"),
            "File not found: [PATH]"
        );
        assert_eq!(
            sanitize_file_paths("Error: \\\\server\\share\\data.txt"),
            "Error: [PATH]"
        );
    }

    #[test]
    fn test_sanitize_ipv4_addresses() {
        assert_eq!(
            sanitize_ip_addresses("Connection to 192.168.1.100 failed"),
            "Connection to [IP] failed"
        );
        assert_eq!(
            sanitize_ip_addresses("Server: 10.0.0.1:8080"),
            "Server: [IP]:8080"
        );
    }

    #[test]
    fn test_sanitize_ipv6_addresses() {
        assert_eq!(
            sanitize_ip_addresses("Failed: 2001:0db8:85a3:0000:0000:8a2e:0370:7334"),
            "Failed: [IP]"
        );
    }

    #[test]
    fn test_sanitize_connection_strings() {
        assert_eq!(
            sanitize_connection_strings("Connect failed: postgres://user:pass@localhost:5432/db"),
            "Connect failed: [CONNECTION]"
        );
        assert_eq!(
            sanitize_connection_strings("Error: mongodb://admin:secret@cluster.example.com/mydb"),
            "Error: [CONNECTION]"
        );
    }

    #[test]
    fn test_sanitize_secrets() {
        assert_eq!(
            sanitize_secrets("API key: api_key=sk_test_1234567890abcdef"),
            "API key: api_key=[REDACTED]"
        );
        assert_eq!(
            sanitize_secrets("Auth failed: token=eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"),
            "Auth failed: token=[REDACTED]"
        );
        assert_eq!(
            sanitize_secrets("Login: password=MySecretPass123"),
            "Login: password=[REDACTED]"
        );
        assert_eq!(
            sanitize_secrets("Header: Authorization: Bearer abc123"),
            "Header: Authorization: bearer=[REDACTED]"
        );
    }

    #[test]
    fn test_sanitize_email_addresses() {
        assert_eq!(
            sanitize_email_addresses("User: admin@example.com"),
            "User: [EMAIL]"
        );
        assert_eq!(
            sanitize_email_addresses("Contact: support@company.org"),
            "Contact: [EMAIL]"
        );
    }

    #[test]
    fn test_sanitize_urls() {
        assert_eq!(
            sanitize_urls("Failed to fetch: https://api.example.com/v1/users"),
            "Failed to fetch: [URL]"
        );
        assert_eq!(
            sanitize_urls("Error: http://internal-service.local/health"),
            "Error: [URL]"
        );
    }

    #[test]
    fn test_full_sanitization() {
        let message = "Connection to postgres://admin:pass@192.168.1.100:5432/db failed. \
                       Check /etc/database/config.yml and contact support@company.com. \
                       API key: api_key=sk_live_abc123";

        let sanitized = sanitize_error_message(message);

        // Should not contain any sensitive info
        assert!(!sanitized.contains("postgres://"));
        assert!(!sanitized.contains("admin:pass"));
        assert!(!sanitized.contains("192.168.1.100"));
        assert!(!sanitized.contains("/etc/database"));
        assert!(!sanitized.contains("support@company.com"));
        assert!(!sanitized.contains("sk_live_abc123"));

        // Should contain redacted markers
        assert!(sanitized.contains("[CONNECTION]"));
        assert!(sanitized.contains("[PATH]"));
        assert!(sanitized.contains("[EMAIL]"));
        assert!(sanitized.contains("[REDACTED]"));
    }

    #[test]
    fn test_sanitized_error_production_mode() {
        let error = std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "File not found: /etc/secrets/api_key.txt",
        );

        let sanitized = SanitizedError::production(error);
        let display = format!("{}", sanitized);

        assert!(!display.contains("/etc/secrets"));
        assert!(display.contains("[PATH]"));
    }

    #[test]
    fn test_sanitized_error_development_mode() {
        let error = std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "File not found: /etc/secrets/api_key.txt",
        );

        let sanitized = SanitizedError::development(error);
        let display = format!("{}", sanitized);

        // In development mode, should show full details
        assert!(display.contains("/etc/secrets/api_key.txt"));
    }

    #[test]
    fn test_display_mode_default() {
        // Default should be production for safety
        assert_eq!(DisplayMode::default(), DisplayMode::Production);
    }

    #[test]
    fn test_generic_error_message() {
        let msg = generic_error();
        assert_eq!(msg, GENERIC_ERROR_MESSAGE);
        assert!(msg.contains("An error occurred"));
    }

    #[test]
    fn test_no_false_positives() {
        // Should not sanitize normal text
        let message = "User 123 requested tool list";
        assert_eq!(sanitize_error_message(message), message);

        // Should not sanitize port numbers
        let message = "Server running on port 8080";
        assert_eq!(sanitize_error_message(message), message);
    }
}
