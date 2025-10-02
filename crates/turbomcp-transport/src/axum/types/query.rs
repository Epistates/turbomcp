//! Query parameter types for HTTP endpoints
//!
//! This module provides query parameter structs for various HTTP endpoints
//! including Server-Sent Events and WebSocket connections.

use serde::Deserialize;

/// Query parameters for SSE endpoint
#[derive(Debug, Deserialize)]
pub struct SseQuery {
    /// Optional session ID for reconnection
    pub session_id: Option<String>,

    /// Last event ID for resumption
    pub last_event_id: Option<String>,
}

/// Query parameters for WebSocket endpoint
#[derive(Debug, Deserialize)]
pub struct WebSocketQuery {
    /// Optional session ID
    pub session_id: Option<String>,

    /// Optional protocol version
    pub protocol: Option<String>,
}