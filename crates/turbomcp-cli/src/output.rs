//! Output formatting for CLI results

use crate::cli::Connection;

/// Format and display CLI output based on connection settings
///
/// # Arguments
/// * `conn` - Connection configuration containing output format preferences
/// * `value` - JSON value to output
///
/// # Output Formats
/// - JSON mode (--json): Pretty-printed JSON
/// - Human mode (default): Compact JSON string
pub fn display(conn: &Connection, value: &serde_json::Value) -> Result<(), String> {
    if conn.json {
        println!(
            "{}",
            serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
        );
    } else {
        println!("{value}");
    }
    Ok(())
}
