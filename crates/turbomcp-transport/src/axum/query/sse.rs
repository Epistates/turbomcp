//! SSE (Server-Sent Events) query parameters
//!
//! This module defines query parameters for the SSE endpoint,
//! supporting session management and event resumption.

use serde::Deserialize;

/// Query parameters for SSE endpoint
#[derive(Debug, Deserialize, Default)]
pub struct SseQuery {
    /// Optional session ID for reconnection
    pub session_id: Option<String>,

    /// Last event ID for resumption
    pub last_event_id: Option<String>,
}

impl SseQuery {
    /// Create new SSE query with session ID
    pub fn with_session(session_id: String) -> Self {
        Self {
            session_id: Some(session_id),
            last_event_id: None,
        }
    }

    /// Create new SSE query with last event ID for resumption
    pub fn with_last_event(last_event_id: String) -> Self {
        Self {
            session_id: None,
            last_event_id: Some(last_event_id),
        }
    }

    /// Check if this is a reconnection request
    pub fn is_reconnection(&self) -> bool {
        self.session_id.is_some()
    }

    /// Check if this is a resumption request
    pub fn is_resumption(&self) -> bool {
        self.last_event_id.is_some()
    }
}
