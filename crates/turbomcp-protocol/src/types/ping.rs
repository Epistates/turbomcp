//! Connection testing types
//!
//! This module contains types for MCP ping functionality,
//! allowing connection health checking between clients and servers.

use serde::{Deserialize, Serialize};

/// Ping request parameters (optional data)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PingParams {
    /// Optional data to echo back
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Ping request wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingRequest {
    /// Ping parameters
    #[serde(flatten)]
    pub params: PingParams,
}

/// Ping result (echoes back the data)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingResult {
    /// Echoed data from the request
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    /// Optional metadata per MCP 2025-06-18 specification
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

impl PingResult {
    /// Create a new ping result
    pub fn new(data: Option<serde_json::Value>) -> Self {
        Self { data, _meta: None }
    }

    /// Create a ping result with no data
    pub fn empty() -> Self {
        Self::new(None)
    }

    /// Add metadata to this result
    pub fn with_meta(mut self, meta: serde_json::Value) -> Self {
        self._meta = Some(meta);
        self
    }
}
