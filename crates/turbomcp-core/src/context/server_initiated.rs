//! Server-initiated communication context types.
//!
//! This module contains types for handling bidirectional communication where
//! the server initiates requests to clients, including sampling, elicitation,
//! and other server-to-client operations.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::capabilities::{CommunicationDirection, CommunicationInitiator, ServerInitiatedType};
use super::client::ClientCapabilities;
use crate::types::Timestamp;

/// Enhanced context for bidirectional MCP communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BidirectionalContext {
    /// Communication direction
    pub direction: CommunicationDirection,
    /// Initiator of the request
    pub initiator: CommunicationInitiator,
    /// Whether response is expected
    pub expects_response: bool,
    /// Parent request ID (for server-initiated requests in response to client requests)
    pub parent_request_id: Option<String>,
    /// Request type for validation
    pub request_type: Option<String>,
    /// Server ID for server-initiated requests
    pub server_id: Option<String>,
    /// Correlation ID for request tracking
    pub correlation_id: String,
    /// Bidirectional communication metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Context for server-initiated requests (sampling, roots listing)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInitiatedContext {
    /// Type of server-initiated request
    pub request_type: ServerInitiatedType,
    /// Originating server ID
    pub server_id: String,
    /// Request correlation ID
    pub correlation_id: String,
    /// Client capabilities
    pub client_capabilities: Option<ClientCapabilities>,
    /// Request timestamp
    pub initiated_at: Timestamp,
    /// Request metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

impl BidirectionalContext {
    /// Create a new bidirectional context
    pub fn new(direction: CommunicationDirection, initiator: CommunicationInitiator) -> Self {
        Self {
            direction,
            initiator,
            expects_response: true,
            parent_request_id: None,
            request_type: None,
            server_id: None,
            correlation_id: Uuid::new_v4().to_string(),
            metadata: HashMap::new(),
        }
    }

    /// Track request direction for proper routing
    pub fn with_direction(mut self, direction: CommunicationDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Set the request type
    pub fn with_request_type(mut self, request_type: String) -> Self {
        self.request_type = Some(request_type);
        self
    }

    /// Validate request direction against protocol rules
    pub fn validate_direction(&self) -> Result<(), String> {
        match (&self.direction, &self.initiator) {
            (CommunicationDirection::ClientToServer, CommunicationInitiator::Client) => Ok(()),
            (CommunicationDirection::ServerToClient, CommunicationInitiator::Server) => Ok(()),
            _ => Err("Invalid direction/initiator combination".to_string()),
        }
    }
}

impl ServerInitiatedContext {
    /// Create a new server-initiated context
    pub fn new(request_type: ServerInitiatedType, server_id: String) -> Self {
        Self {
            request_type,
            server_id,
            correlation_id: Uuid::new_v4().to_string(),
            client_capabilities: None,
            initiated_at: Timestamp::now(),
            metadata: HashMap::new(),
        }
    }

    /// Set client capabilities
    pub fn with_capabilities(mut self, capabilities: ClientCapabilities) -> Self {
        self.client_capabilities = Some(capabilities);
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: String, value: serde_json::Value) -> Self {
        self.metadata.insert(key, value);
        self
    }
}
