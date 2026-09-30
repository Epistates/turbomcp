//! The service-layer error type. A user [`McpError`](turbomcp_core::McpError)
//! becomes a wire error through `McpError::to_jsonrpc_error`, in core.

use turbomcp_core::codec::CodecError;
use turbomcp_core::{JsonRpcError, JsonRpcResponse, RequestId};

/// Errors at the service/transport boundary — *not* normal protocol responses.
///
/// A user handler returning `Err(McpError)` is **not** a `ProtocolError`: it
/// becomes a JSON-RPC error response inside the `Ok` arm of the service (see
/// `McpError::to_jsonrpc_error`), and so do protocol refusals such as an
/// unsupported version. `ProtocolError` is for what fails around the request:
/// a malformed frame, an unknown session, a dead transport, shutdown, or a
/// failure in the machinery itself.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ProtocolError {
    /// A frame could not be parsed (JSON-RPC `-32700`).
    #[error("parse error: {0}")]
    Parse(String),
    /// The request referenced a session this server does not know — expired,
    /// evicted, or never created. Per the `2025-11-25` Streamable HTTP spec the
    /// HTTP transport answers this with `404 Not Found`, prompting the client
    /// to re-`initialize`.
    #[error("unknown session: {0}")]
    UnknownSession(String),
    /// The underlying transport failed (connection closed, I/O error).
    #[error("transport error: {0}")]
    Transport(String),
    /// The server is draining and will not accept new work.
    #[error("server is shutting down")]
    ServerShuttingDown,
    /// An unexpected internal failure (`-32603`).
    #[error("internal error: {0}")]
    Internal(String),
}

impl ProtocolError {
    /// The JSON-RPC error code for this condition.
    #[must_use]
    pub fn jsonrpc_code(&self) -> i32 {
        match self {
            Self::Parse(_) => turbomcp_core::codes::PARSE_ERROR,
            Self::UnknownSession(_) => turbomcp_core::codes::NO_ACTIVE_SESSION,
            // `-32000` is the implementation-defined floor.
            Self::Transport(_) | Self::ServerShuttingDown => turbomcp_core::codes::SERVER_ERROR,
            Self::Internal(_) => turbomcp_core::codes::INTERNAL_ERROR,
        }
    }

    /// Render this error as a JSON-RPC error response for `id`.
    ///
    /// Used when the error is still answerable on the wire (e.g. version
    /// mismatch on a request). Pure transport death has no response.
    #[must_use]
    pub fn into_response(self, id: RequestId) -> JsonRpcResponse {
        JsonRpcResponse::error(id, self.to_jsonrpc_error())
    }

    /// This error as a JSON-RPC error object.
    #[must_use]
    pub fn to_jsonrpc_error(&self) -> JsonRpcError {
        JsonRpcError {
            code: self.jsonrpc_code(),
            message: self.to_string(),
            data: None,
        }
    }
}

impl From<CodecError> for ProtocolError {
    fn from(e: CodecError) -> Self {
        // Both encode and decode failures surface as parse errors at the
        // protocol boundary — the frame could not be turned into/from a value.
        ProtocolError::Parse(e.to_string())
    }
}
