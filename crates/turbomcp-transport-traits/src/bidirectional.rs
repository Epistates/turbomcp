//! Bidirectional transport utilities
//!
//! Provides types for bidirectional MCP communication patterns including
//! server-initiated requests and message correlation.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::TransportMessage;

/// Message direction in the transport layer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageDirection {
    /// Client to server message
    ClientToServer,
    /// Server to client message
    ServerToClient,
}

/// Correlation context for request-response patterns
#[derive(Debug)]
pub struct CorrelationContext {
    /// Unique correlation ID
    pub correlation_id: String,
    /// Original request message ID
    pub request_id: String,
    /// Response channel (not cloneable, so we don't derive Clone)
    pub response_tx: Option<oneshot::Sender<TransportMessage>>,
    /// Timeout duration
    pub timeout: Duration,
    /// Creation timestamp
    pub created_at: std::time::Instant,
}

impl CorrelationContext {
    /// Create a new correlation context
    pub fn new(
        correlation_id: String,
        request_id: String,
        response_tx: oneshot::Sender<TransportMessage>,
        timeout: Duration,
    ) -> Self {
        Self {
            correlation_id,
            request_id,
            response_tx: Some(response_tx),
            timeout,
            created_at: std::time::Instant::now(),
        }
    }

    /// Check if this correlation has expired
    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() > self.timeout
    }

    /// Take the response sender, if available
    pub fn take_sender(&mut self) -> Option<oneshot::Sender<TransportMessage>> {
        self.response_tx.take()
    }
}

/// Connection state for bidirectional communication
#[derive(Debug, Clone, Default)]
pub struct ConnectionState {
    /// Whether server-initiated requests are enabled
    pub server_initiated_enabled: bool,
    /// Active server-initiated request IDs
    pub active_server_requests: Vec<String>,
    /// Pending elicitations
    pub pending_elicitations: Vec<String>,
    /// Connection metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

impl ConnectionState {
    /// Create a new connection state
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable server-initiated requests
    pub fn enable_server_initiated(&mut self) {
        self.server_initiated_enabled = true;
    }

    /// Add an active server request
    pub fn add_server_request(&mut self, request_id: String) {
        self.active_server_requests.push(request_id);
    }

    /// Remove a completed server request
    pub fn remove_server_request(&mut self, request_id: &str) {
        self.active_server_requests.retain(|id| id != request_id);
    }

    /// Add a pending elicitation
    pub fn add_elicitation(&mut self, elicitation_id: String) {
        self.pending_elicitations.push(elicitation_id);
    }

    /// Remove a completed elicitation
    pub fn remove_elicitation(&mut self, elicitation_id: &str) {
        self.pending_elicitations.retain(|id| id != elicitation_id);
    }

    /// Set connection metadata
    pub fn set_metadata(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.metadata.insert(key.into(), value);
    }

    /// Get connection metadata
    pub fn get_metadata(&self, key: &str) -> Option<&serde_json::Value> {
        self.metadata.get(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_state_default() {
        let state = ConnectionState::default();
        assert!(!state.server_initiated_enabled);
        assert!(state.active_server_requests.is_empty());
        assert!(state.pending_elicitations.is_empty());
        assert!(state.metadata.is_empty());
    }

    #[test]
    fn test_connection_state_server_requests() {
        let mut state = ConnectionState::new();
        state.enable_server_initiated();
        assert!(state.server_initiated_enabled);

        state.add_server_request("req-1".to_string());
        state.add_server_request("req-2".to_string());
        assert_eq!(state.active_server_requests.len(), 2);

        state.remove_server_request("req-1");
        assert_eq!(state.active_server_requests.len(), 1);
        assert_eq!(state.active_server_requests[0], "req-2");
    }

    #[test]
    fn test_connection_state_elicitations() {
        let mut state = ConnectionState::new();

        state.add_elicitation("elic-1".to_string());
        assert_eq!(state.pending_elicitations.len(), 1);

        state.remove_elicitation("elic-1");
        assert!(state.pending_elicitations.is_empty());
    }

    #[test]
    fn test_connection_state_metadata() {
        let mut state = ConnectionState::new();

        state.set_metadata("key1", serde_json::json!("value1"));
        assert_eq!(
            state.get_metadata("key1"),
            Some(&serde_json::json!("value1"))
        );
        assert_eq!(state.get_metadata("nonexistent"), None);
    }

    #[test]
    fn test_message_direction() {
        assert_ne!(
            MessageDirection::ClientToServer,
            MessageDirection::ServerToClient
        );

        // Test serialization
        let json = serde_json::to_string(&MessageDirection::ClientToServer).unwrap();
        let deserialized: MessageDirection = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, MessageDirection::ClientToServer);
    }
}
