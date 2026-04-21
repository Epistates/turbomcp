//! MCP protocol wire-wrapper types.
//!
//! Request/response/notification envelopes exchanged over the wire by MCP
//! clients and servers. These are the canonical definitions; `turbomcp-protocol`
//! re-exports them verbatim.
//!
//! Kept `no_std + alloc` compatible so WASM and embedded consumers can depend
//! on the same types as native transports.

#[cfg(not(feature = "std"))]
use alloc::{string::String, vec, vec::Vec};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::content::{Content, PromptMessage};
use crate::definitions::Implementation;
use crate::protocol::{ClientCapabilities, ServerCapabilities};

// =============================================================================
// Initialization handshake
// =============================================================================

/// The `initialize` request sent by the client as the first message after connection.
///
/// Used to exchange capabilities and agree on a protocol version for the session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeRequest {
    /// The protocol version the client wishes to use.
    #[serde(rename = "protocolVersion")]
    pub protocol_version: crate::primitives::ProtocolVersion,
    /// The capabilities supported by the client.
    pub capabilities: ClientCapabilities,
    /// Information about the client's implementation (e.g., name, version).
    #[serde(rename = "clientInfo")]
    pub client_info: Implementation,
    /// Optional metadata for the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
}

/// The response to a successful `initialize` request.
///
/// Server confirms connection parameters and declares its own capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    /// The protocol version that will be used for the session, chosen by the server.
    #[serde(rename = "protocolVersion")]
    pub protocol_version: crate::primitives::ProtocolVersion,
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
    pub _meta: Option<Value>,
}

/// Sent by the client after a successful `InitializeResult` to confirm readiness.
///
/// This notification has no parameters.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InitializedNotification {}

// =============================================================================
// Tool invocation
// =============================================================================

/// The result of a `CallToolRequest`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CallToolResult {
    /// The output of the tool as a series of content blocks. Required.
    pub content: Vec<Content>,
    /// Whether the tool execution resulted in an error.
    ///
    /// When `true`, all content blocks should be treated as error information;
    /// the message may span multiple text blocks for structured error reporting.
    #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    /// Optional structured output conforming to the tool's `output_schema`.
    ///
    /// Tools that emit structured content SHOULD also include the serialized
    /// JSON in a `TextContent` block for clients that don't support structured
    /// output.
    #[serde(rename = "structuredContent", skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
    /// Optional metadata for the result.
    ///
    /// For client applications and tools to pass context that should NOT be
    /// exposed to LLMs (tracking IDs, metrics, cache status, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
    /// Optional task ID when tool execution is augmented with task tracking
    /// (MCP 2025-11-25 draft — SEP-1686).
    ///
    /// When a tool call includes task metadata, the server creates a task to
    /// track the operation and returns the task_id here. Clients use this to
    /// monitor progress via `tasks/get` or retrieve final results via
    /// `tasks/result`.
    #[serde(rename = "taskId", skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

impl CallToolResult {
    /// Create a successful result with a single text content block.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![Content::text(text)],
            ..Default::default()
        }
    }

    /// Create an error result with a single text content block and `is_error = true`.
    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: vec![Content::text(message)],
            is_error: Some(true),
            ..Default::default()
        }
    }

    /// Create a successful JSON result (pretty-printed text content).
    pub fn json<T: Serialize>(value: &T) -> Result<Self, serde_json::Error> {
        let text = serde_json::to_string_pretty(value)?;
        Ok(Self::text(text))
    }

    /// Create a result with multiple content items.
    #[must_use]
    pub fn contents(contents: Vec<Content>) -> Self {
        Self {
            content: contents,
            ..Default::default()
        }
    }

    /// Create an image result (base64-encoded).
    #[must_use]
    pub fn image(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self {
            content: vec![Content::image(data, mime_type)],
            ..Default::default()
        }
    }

    /// Extracts and concatenates all text content (newline-joined).
    ///
    /// Returns an empty string if no text blocks are present.
    pub fn all_text(&self) -> String {
        let texts: Vec<&str> = self.content.iter().filter_map(Content::as_text).collect();
        texts.join("\n")
    }

    /// Returns the text of the first text block, if any.
    pub fn first_text(&self) -> Option<&str> {
        self.content.first().and_then(Content::as_text)
    }

    /// Whether `is_error` is explicitly `true`.
    pub fn has_error(&self) -> bool {
        self.is_error.unwrap_or(false)
    }
}

// =============================================================================
// Prompt retrieval
// =============================================================================

/// The result of a `prompts/get` request.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GetPromptResult {
    /// Optional description of this prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The sequence of messages that compose the prompt.
    pub messages: Vec<PromptMessage>,
    /// Optional metadata for the result.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Value>,
}
