//! Elicitation context types for server-initiated user input requests.
//!
//! This module contains types for handling elicitation requests where the server
//! needs to prompt the user for additional information during request processing.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::client::ClientSession;
use crate::types::Timestamp;

/// Context for server-initiated elicitation (user input) requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationContext {
    /// Unique elicitation request ID
    pub elicitation_id: String,
    /// Message presented to user
    pub message: String,
    /// Schema for user input validation (using protocol ElicitationSchema when available)
    pub schema: serde_json::Value,
    /// Input constraints and hints
    pub constraints: Option<serde_json::Value>,
    /// Default values for fields
    pub defaults: Option<HashMap<String, serde_json::Value>>,
    /// Whether input is required or optional
    pub required: bool,
    /// Timeout for user response in milliseconds
    pub timeout_ms: Option<u64>,
    /// Cancellation support
    pub cancellable: bool,
    /// Client session information
    pub client_session: Option<ClientSession>,
    /// Timestamp of elicitation request
    pub requested_at: Timestamp,
    /// Current elicitation state
    pub state: ElicitationState,
    /// Custom elicitation metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// State of an elicitation request
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ElicitationState {
    /// Waiting for user response
    Pending,
    /// User provided input
    Accepted,
    /// User explicitly declined
    Declined,
    /// User cancelled/dismissed
    Cancelled,
    /// Response timeout exceeded
    TimedOut,
}

impl ElicitationContext {
    /// Create a new elicitation context
    pub fn new(message: String, schema: serde_json::Value) -> Self {
        Self {
            elicitation_id: Uuid::new_v4().to_string(),
            message,
            schema,
            constraints: None,
            defaults: None,
            required: true,
            timeout_ms: Some(30000),
            cancellable: true,
            client_session: None,
            requested_at: Timestamp::now(),
            state: ElicitationState::Pending,
            metadata: HashMap::new(),
        }
    }

    /// Set the client session
    pub fn with_client_session(mut self, session: ClientSession) -> Self {
        self.client_session = Some(session);
        self
    }

    /// Set the timeout
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }

    /// Update the state
    pub fn set_state(&mut self, state: ElicitationState) {
        self.state = state;
    }

    /// Check if elicitation is complete
    pub fn is_complete(&self) -> bool {
        !matches!(self.state, ElicitationState::Pending)
    }
}
