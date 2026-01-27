//! Logging and progress types
//!
//! This module contains types for MCP logging and progress notifications.

use serde::{Deserialize, Serialize};

/// Log level enumeration
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Debug level
    Debug,
    /// Info level
    Info,
    /// Notice level
    Notice,
    /// Warning level
    Warning,
    /// Error level
    Error,
    /// Critical level
    Critical,
    /// Alert level
    Alert,
    /// Emergency level
    Emergency,
}

/// Set logging level request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetLevelRequest {
    /// Log level to set
    pub level: LogLevel,
}

/// Set logging level result (empty)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetLevelResult {}

/// Logging notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingNotification {
    /// Log level
    pub level: LogLevel,
    /// Log message
    pub data: serde_json::Value,
    /// Optional logger name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logger: Option<String>,
}

/// Progress notification for reporting progress on long-running operations.
///
/// Servers can send progress notifications to clients to update them on
/// the status of operations. The `progress_token` should match the token
/// provided in the original request's `_meta` field.
///
/// # Example
///
/// ```rust
/// use turbomcp_protocol::types::ProgressNotification;
///
/// let notification = ProgressNotification {
///     progress_token: "request-123".to_string(),
///     progress: 50,
///     total: Some(100),
///     message: Some("Processing files...".to_string()),
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressNotification {
    /// Token identifying the request this progress is for.
    /// This should match the `progressToken` from the request's `_meta` field.
    #[serde(rename = "progressToken")]
    pub progress_token: String,

    /// Current progress value.
    pub progress: u64,

    /// Optional total value (for percentage calculation).
    /// If provided, progress/total gives the completion percentage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,

    /// Optional human-readable progress message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}
