//! Ping and health monitoring context types.
//!
//! This module contains types for handling ping requests, health checks,
//! and connection quality monitoring in MCP systems.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::capabilities::PingOrigin;

/// Context for ping/health monitoring requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingContext {
    /// Ping origin (client or server)
    pub origin: PingOrigin,
    /// Expected response time threshold in milliseconds
    pub response_threshold_ms: Option<u64>,
    /// Custom ping payload
    pub payload: Option<serde_json::Value>,
    /// Health check metadata
    pub health_metadata: HashMap<String, serde_json::Value>,
    /// Connection quality metrics
    pub connection_metrics: Option<ConnectionMetrics>,
}

/// Connection quality metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionMetrics {
    /// Round-trip time in milliseconds
    pub rtt_ms: Option<f64>,
    /// Packet loss percentage (0.0-100.0)
    pub packet_loss: Option<f64>,
    /// Connection uptime in seconds
    pub uptime_seconds: Option<u64>,
    /// Bytes sent
    pub bytes_sent: Option<u64>,
    /// Bytes received
    pub bytes_received: Option<u64>,
    /// Last successful communication timestamp
    pub last_success: Option<DateTime<Utc>>,
}
