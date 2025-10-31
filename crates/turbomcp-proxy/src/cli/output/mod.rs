//! Output formatters for different formats
//!
//! This module provides a trait-based output system that supports
//! multiple formats (human-readable, JSON, YAML) with automatic
//! format selection based on TTY detection.

pub mod human;
pub mod json;

use clap::ValueEnum;
use std::io::Write;

use crate::error::ProxyResult;
use crate::introspection::ServerSpec;

/// Output format for CLI results
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable colored output (default)
    Human,
    /// JSON format (for scripting)
    Json,
    /// JSON with pretty-printing
    JsonPretty,
    /// YAML format
    Yaml,
}

/// Trait for formatting and outputting `ServerSpec` results
pub trait OutputFormatter {
    /// Format and write the server specification
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if writing to the output fails.
    fn write_spec(&self, spec: &ServerSpec, writer: &mut dyn Write) -> ProxyResult<()>;

    /// Format and write an error message
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if writing to the output fails.
    fn write_error(&self, error: &str, writer: &mut dyn Write) -> ProxyResult<()>;

    /// Format and write a success message
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if writing to the output fails.
    fn write_success(&self, message: &str, writer: &mut dyn Write) -> ProxyResult<()>;
}

/// Factory function to create the appropriate formatter
#[must_use]
#[allow(clippy::match_same_arms)]
pub fn get_formatter(format: OutputFormat) -> Box<dyn OutputFormatter> {
    match format {
        OutputFormat::Human => Box::new(human::HumanFormatter::new()),
        OutputFormat::Json => Box::new(json::JsonFormatter::new(false)),
        OutputFormat::JsonPretty => Box::new(json::JsonFormatter::new(true)),
        OutputFormat::Yaml => Box::new(json::JsonFormatter::new(true)), // TODO: Implement YAML
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_formatter_creation() {
        let _formatter = get_formatter(OutputFormat::Human);
        let _formatter = get_formatter(OutputFormat::Json);
        let _formatter = get_formatter(OutputFormat::JsonPretty);
    }
}
