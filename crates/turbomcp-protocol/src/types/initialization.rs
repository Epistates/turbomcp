//! Types for the MCP connection initialization and handshake process.
//!
//! This module defines the data structures used during the initial handshake
//! between an MCP client and server, where they negotiate capabilities and
//! exchange implementation details.

use serde::{Deserialize, Serialize};

use super::{
    capabilities::{ClientCapabilities, ServerCapabilities},
    core::{Implementation, ProtocolVersion},
};

/// The `initialize` request is sent by the client as the first message after connection.
///
/// It allows the client and server to exchange their capabilities and agree on a
/// protocol version to use for the duration of the session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeRequest {
    /// The protocol version the client wishes to use.
    #[serde(rename = "protocolVersion")]
    pub protocol_version: ProtocolVersion,
    /// The capabilities supported by the client.
    pub capabilities: ClientCapabilities,
    /// Information about the client's implementation (e.g., name, version).
    #[serde(rename = "clientInfo")]
    pub client_info: Implementation,
    /// Optional metadata for the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// The response to a successful `initialize` request.
///
/// The server sends this message to confirm the connection parameters and
/// to declare its own capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    /// The protocol version that will be used for the session, chosen by the server.
    #[serde(rename = "protocolVersion")]
    pub protocol_version: ProtocolVersion,
    /// The capabilities supported by the server.
    pub capabilities: ServerCapabilities,
    /// Information about the server's implementation (e.g., name, version).
    #[serde(rename = "serverInfo")]
    pub server_info: Implementation,
    /// Optional human-readable instructions for the client.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Optional metadata for the result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// A notification sent from the client to the server after receiving a successful
/// `InitializeResult`, confirming that the client is ready to proceed.
///
/// This notification has no parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializedNotification {}
