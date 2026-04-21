//! Communication-direction enums used by the bidirectional MCP flow.
//!
//! The old `ServerToClientRequests` trait was deleted in v3.2: no crate in
//! the workspace implemented it, and its job (sampling / elicitation /
//! notifications) is now handled by the unified session model — see
//! [`turbomcp_core::McpSession`] plus the `sample()` / `elicit_form()` /
//! `elicit_url()` / `notify_client()` methods on
//! [`turbomcp_core::RequestContext`].
//!
//! The supporting enums in this module remain useful as public vocabulary
//! for describing request/response direction in auditing and analytics.

use serde::{Deserialize, Serialize};

/// Communication direction for bidirectional requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommunicationDirection {
    /// Client to server.
    ClientToServer,
    /// Server to client.
    ServerToClient,
}

/// Communication initiator for tracking request origins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommunicationInitiator {
    /// Client initiated the request.
    Client,
    /// Server initiated the request.
    Server,
}

/// Types of server-initiated requests.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServerInitiatedType {
    /// Sampling / message creation request.
    Sampling,
    /// Elicitation request for user input.
    Elicitation,
    /// Roots listing request.
    Roots,
    /// Ping / health check request.
    Ping,
}

/// Origin of a ping request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PingOrigin {
    /// Client initiated ping.
    Client,
    /// Server initiated ping.
    Server,
}
