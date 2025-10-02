//! Connection initialization and handshake types
//!
//! This module contains types for the MCP connection handshake process,
//! including initialization requests, responses, and the initialized notification.

use serde::{Deserialize, Serialize};

use super::{
    capabilities::{ClientCapabilities, ServerCapabilities},
    core::{Implementation, ProtocolVersion},
};

/// Initialize request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeRequest {
    /// Protocol version
    #[serde(rename = "protocolVersion")]
    pub protocol_version: ProtocolVersion,
    /// Client capabilities
    pub capabilities: ClientCapabilities,
    /// Client implementation info
    #[serde(rename = "clientInfo")]
    pub client_info: Implementation,
    /// Optional metadata per MCP 2025-06-18 specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// Initialize result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    /// Protocol version
    #[serde(rename = "protocolVersion")]
    pub protocol_version: ProtocolVersion,
    /// Server capabilities
    pub capabilities: ServerCapabilities,
    /// Server implementation info
    #[serde(rename = "serverInfo")]
    pub server_info: Implementation,
    /// Additional instructions for the client
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Optional metadata per MCP 2025-06-18 specification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

/// Initialized notification (no parameters)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializedNotification;
