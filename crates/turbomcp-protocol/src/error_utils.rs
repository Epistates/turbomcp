//! Error handling utility functions for consistent error patterns
//!
//! This module provides standardized error conversion and handling patterns
//! to eliminate inconsistencies across the codebase.

use std::fmt;

/// Standard error conversion pattern for consistent error formatting
pub trait StandardErrorConversion<T> {
    /// Convert error to standard string format with context
    fn to_standard_error(self, context: &str) -> Result<T, String>;
}

impl<T, E: fmt::Display> StandardErrorConversion<T> for Result<T, E> {
    fn to_standard_error(self, context: &str) -> Result<T, String> {
        self.map_err(|e| format!("{context}: {e}"))
    }
}

/// Convenience function for consistent JSON parsing errors
pub fn json_parse_error<T>(
    result: Result<T, serde_json::Error>,
    context: &str,
) -> Result<T, String> {
    result.map_err(|e| format!("{context}: {e}"))
}

/// Convenience function for consistent I/O errors  
pub fn io_error<T>(result: Result<T, std::io::Error>, context: &str) -> Result<T, String> {
    result.map_err(|e| format!("{context}: {e}"))
}

/// Convenience function for consistent network errors
pub fn network_error<T>(
    result: Result<T, Box<dyn std::error::Error + Send + Sync>>,
    context: &str,
) -> Result<T, String> {
    result.map_err(|e| format!("{context}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn test_standard_error_conversion() {
        let result: Result<i32, &str> = Err("original error");
        let converted = result.to_standard_error("Operation failed");
        assert_eq!(converted.unwrap_err(), "Operation failed: original error");
    }

    #[test]
    fn test_json_parse_error() {
        let invalid_json = "{ invalid json";
        let result: Result<serde_json::Value, _> = serde_json::from_str(invalid_json);
        let converted = json_parse_error(result, "Failed to parse JSON");
        assert!(converted.unwrap_err().starts_with("Failed to parse JSON:"));
    }

    #[test]
    fn test_io_error() {
        let result: Result<String, std::io::Error> = Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        let converted = io_error(result, "Failed to read file");
        assert_eq!(
            converted.unwrap_err(),
            "Failed to read file: file not found"
        );
    }
}
