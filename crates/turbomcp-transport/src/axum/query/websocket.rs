//! WebSocket query parameters
//!
//! This module defines query parameters for WebSocket connections,
//! supporting protocol negotiation and session management.

#[cfg(feature = "http")]
use serde::Deserialize;

#[cfg(feature = "http")]
/// Query parameters for WebSocket endpoint
#[derive(Debug, Deserialize)]
pub struct WebSocketQuery {
    /// Optional session ID
    pub session_id: Option<String>,

    /// Optional protocol version
    pub protocol: Option<String>,
}

#[cfg(feature = "http")]
impl Default for WebSocketQuery {
    fn default() -> Self {
        Self {
            session_id: None,
            protocol: Some("2025-06-18".to_string()),
        }
    }
}

#[cfg(feature = "http")]
impl WebSocketQuery {
    /// Create new WebSocket query with session ID
    pub fn with_session(session_id: String) -> Self {
        Self {
            session_id: Some(session_id),
            protocol: Some("2025-06-18".to_string()),
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
        self.protocol.as_deref().unwrap_or("2025-06-18")
    }

    /// Check if this includes a session ID
    pub fn has_session(&self) -> bool {
        self.session_id.is_some()
    }
}
