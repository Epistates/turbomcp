//! Server capabilities and communication types for bidirectional MCP communication.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Trait for server-initiated requests (sampling, elicitation, roots)
/// This provides a type-safe way for tools to make requests to clients
pub trait ServerCapabilities: Send + Sync + fmt::Debug {
    /// Send a sampling/createMessage request to the client
    fn create_message(
        &self,
        request: serde_json::Value,
    ) -> futures::future::BoxFuture<
        '_,
        Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>>,
    >;

    /// Send an elicitation request to the client
    fn elicit(
        &self,
        request: serde_json::Value,
    ) -> futures::future::BoxFuture<
        '_,
        Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>>,
    >;

    /// List client's root capabilities
    fn list_roots(
        &self,
    ) -> futures::future::BoxFuture<
        '_,
        Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>>,
    >;
}

/// Communication direction for bidirectional requests
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommunicationDirection {
    /// Client to server
    ClientToServer,
    /// Server to client
    ServerToClient,
}

/// Communication initiator for tracking request origins
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommunicationInitiator {
    /// Client initiated the request
    Client,
    /// Server initiated the request
    Server,
}

/// Types of server-initiated requests
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServerInitiatedType {
    /// Sampling/message creation request
    Sampling,
    /// Elicitation request for user input
    Elicitation,
    /// Roots listing request
    Roots,
    /// Ping/health check request
    Ping,
}

/// Origin of a ping request
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PingOrigin {
    /// Client initiated ping
    Client,
    /// Server initiated ping
    Server,
}
