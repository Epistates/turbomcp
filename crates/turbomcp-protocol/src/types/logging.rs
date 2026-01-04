//! Logging types
//!
//! This module contains types for MCP logging notifications.

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
