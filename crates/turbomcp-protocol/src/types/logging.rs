//! Logging and progress types
//!
//! This module contains types for MCP logging and progress notifications.

use serde::{Deserialize, Serialize};

use super::core::ProgressToken;

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

/// Logging notification (sent by server on `notifications/message`).
///
/// Spec name is `LoggingMessageNotification` (`schema.ts:1551`); the local
/// `LoggingNotification` alias predates the spec rename. Both names refer
/// to the same wire shape — see also [`LoggingMessageNotification`] for the
/// spec-faithful name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingNotification {
    /// Log level
    pub level: LogLevel,
    /// Log message
    pub data: serde_json::Value,
    /// Optional logger name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logger: Option<String>,
    /// Optional metadata (`LoggingMessageNotificationParams._meta`).
    ///
    /// Without this field serde silently discarded anything a peer attached,
    /// and the loss was irrecoverable — the notification had already been
    /// deserialized by the time a handler saw it.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// Spec-faithful name for `notifications/message` — same wire shape as
/// [`LoggingNotification`] (kept as an alias for backwards compatibility).
pub type LoggingMessageNotification = LoggingNotification;

/// Progress notification for reporting progress on long-running operations.
///
/// Servers can send progress notifications to clients to update them on
/// the status of operations. The `progress_token` should match the token
/// provided in the original request's `_meta` field.
///
/// Per MCP 2025-11-25 (`schema.ts:23`, `:1551-1561`):
/// - `progressToken` is `string | number` — see [`ProgressToken`].
/// - `progress` and `total` are JSON numbers (floats); they MAY express
///   fractional progress (e.g., `45.7 / 100.0`).
///
/// # Example
///
/// ```rust
/// use turbomcp_protocol::types::{ProgressNotification, ProgressToken};
///
/// let notification = ProgressNotification {
///     progress_token: ProgressToken::from("request-123"),
///     progress: 50.0,
///     total: Some(100.0),
///     message: Some("Processing files...".to_string()),
///     _meta: None,
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressNotification {
    /// Token identifying the request this progress is for.
    /// This should match the `progressToken` from the request's `_meta` field.
    #[serde(rename = "progressToken")]
    pub progress_token: ProgressToken,

    /// Current progress value (JSON number; floats permitted).
    pub progress: f64,

    /// Optional total value (for percentage calculation).
    /// If provided, progress/total gives the completion percentage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<f64>,

    /// Optional human-readable progress message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// Optional metadata (`ProgressNotificationParams._meta`).
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}
