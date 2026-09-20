//! Client configuration types and utilities
//!
//! This module contains configuration structures for MCP client initialization
//! results. The `ConnectionConfig` type lives in the crate root (`crate::ConnectionConfig`).

use turbomcp_protocol::types::ServerCapabilities;

/// Result of client initialization containing server information
///
/// `#[non_exhaustive]`: the handshake result grows with the wire, and every
/// addition would otherwise break callers who build this by struct literal.
/// Construct it from [`crate::Client::initialize`] and read the fields.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct InitializeResult {
    /// Information about the server
    pub server_info: turbomcp_protocol::types::Implementation,

    /// Capabilities supported by the server
    pub server_capabilities: ServerCapabilities,

    /// Protocol version the server chose for this session.
    ///
    /// Already validated against the versions this client speaks — an
    /// unsupported one fails the handshake rather than reaching here.
    pub protocol_version: String,

    /// Server-authored usage guidance, when the server sent any.
    ///
    /// The spec describes this as instructions "describing how to use the
    /// server and its features" that MAY be added to the model's system
    /// prompt. It is the one handshake field written for the model rather than
    /// for the client.
    pub instructions: Option<String>,
}

impl InitializeResult {
    /// Build a handshake result.
    ///
    /// Normally you get one from [`crate::Client::initialize`]; this exists for
    /// tests and for bridges that synthesise a handshake, which `#[non_exhaustive]`
    /// otherwise leaves no way to construct.
    #[must_use]
    pub fn new(
        server_info: turbomcp_protocol::types::Implementation,
        server_capabilities: ServerCapabilities,
        protocol_version: impl Into<String>,
    ) -> Self {
        Self {
            server_info,
            server_capabilities,
            protocol_version: protocol_version.into(),
            instructions: None,
        }
    }

    /// Attach the server's usage guidance.
    #[must_use]
    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }
}
