//! WebSocket query parameters
//!
//! This module defines query parameters for WebSocket connections,
//! supporting protocol negotiation and session management.

use serde::Deserialize;
use turbomcp_protocol::PROTOCOL_VERSION;

/// Query parameters for WebSocket endpoint
#[derive(Debug, Deserialize)]
pub struct WebSocketQuery {
    /// Optional session ID
    pub session_id: Option<String>,

    /// Optional protocol version
    pub protocol: Option<String>,
}

impl Default for WebSocketQuery {
    fn default() -> Self {
        Self {
            session_id: None,
            protocol: Some(PROTOCOL_VERSION.to_string()),
        }
    }
}

impl WebSocketQuery {
    /// Create new WebSocket query with session ID
    pub fn with_session(session_id: String) -> Self {
        Self {
            session_id: Some(session_id),
            protocol: Some(PROTOCOL_VERSION.to_string()),
        }
    }

    /// Create new WebSocket query with specific protocol version
    pub fn with_protocol(protocol: String) -> Self {
        Self {
            session_id: None,
            protocol: Some(protocol),
        }
    }

    /// Get the protocol version, defaulting to MCP protocol version
    pub fn get_protocol(&self) -> &str {
        self.protocol.as_deref().unwrap_or(PROTOCOL_VERSION)
    }

    /// Check if this includes a session ID
    pub fn has_session(&self) -> bool {
        self.session_id.is_some()
    }
}
