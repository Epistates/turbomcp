//! Logging and progress tracking types
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
pub struct SetLevelResult;

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

/// Progress notification per MCP 2025-06-18 specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressNotification {
    /// Progress token from the original request
    #[serde(rename = "progressToken")]
    pub progress_token: ProgressToken,
    /// Current progress value (MUST increase with each notification)
    pub progress: f64,
    /// Total value (MAY be floating point, omit if unknown)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<f64>,
    /// Human-readable progress message (SHOULD provide relevant information)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

// Note: CancelledNotification moved to requests.rs to avoid duplicate exports
