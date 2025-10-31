//! User-friendly error formatting for CLI
//!
//! Converts technical errors into human-readable messages with helpful
//! suggestions and context.

use colored::Colorize;

use crate::error::ProxyError;

/// Format an error for CLI display
#[must_use]
pub fn format_error(error: &ProxyError) -> String {
    match error {
        ProxyError::Backend { message, .. } => {
            format!(
                "{} Backend error\n  {}\n\n{}\n  {}",
                "✗".red().bold(),
                message,
                "Suggestion:".yellow(),
                "Check that the server command is correct and the server starts successfully"
            )
        }
        ProxyError::Configuration { message, .. } => {
            format!(
                "{} Configuration error\n  {}\n\n{}\n  {}",
                "✗".red().bold(),
                message,
                "Suggestion:".yellow(),
                "Run with --help to see all available options"
            )
        }
        ProxyError::Introspection { message, .. } => {
            format!(
                "{} Introspection error\n  {}\n\n{}\n  {}",
                "✗".red().bold(),
                message,
                "Suggestion:".yellow(),
                "Ensure the server implements MCP protocol 2025-06-18 correctly"
            )
        }
        ProxyError::Serialization(err) => {
            format!(
                "{} JSON parsing error\n  {}\n\n{}\n  {}",
                "✗".red().bold(),
                err,
                "Suggestion:".yellow(),
                "The server may be returning invalid JSON. Check server output."
            )
        }
        ProxyError::Io(err) => {
            format!(
                "{} I/O error\n  {}\n\n{}\n  {}",
                "✗".red().bold(),
                err,
                "Suggestion:".yellow(),
                "Check file permissions and disk space"
            )
        }
        _ => format!("{} {}", "✗".red().bold(), error),
    }
}

/// Display an error to stderr and return exit code
#[must_use]
pub fn display_error(error: &ProxyError) -> i32 {
    eprintln!("{}", format_error(error));
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_formatting() {
        let error = ProxyError::configuration("test error");
        let formatted = format_error(&error);
        assert!(formatted.contains("Configuration error"));
        assert!(formatted.contains("test error"));
    }
}
