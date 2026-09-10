//! Client-side error model (`ClientCallError` in PLAN §4.10).
//!
//! A client call fails in one of a few visible ways: the peer answered with a
//! JSON-RPC error, the connection went away, the call timed out, or a
//! successful result didn't deserialize into the type the typed API expected.
//! These are distinct enough to branch on, so they're separate variants rather
//! than one stringly error.

use turbomcp_core::JsonRpcError;

/// The result of a client RPC.
pub type ClientResult<T> = Result<T, ClientError>;

/// A failure issuing or completing a client request.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ClientError {
    /// HTTP failure including a protocol error and authorization/retry headers.
    #[error(transparent)]
    Http(Box<turbomcp_service::HttpFailure>),
    /// The server answered with a JSON-RPC error object.
    #[error("server error {}: {}", .0.code, .0.message)]
    Rpc(JsonRpcError),

    /// The connection is gone — the transport closed or the connection actor
    /// stopped — so the request can never complete.
    #[error("client connection closed")]
    Closed,

    /// No response arrived within the configured request timeout.
    #[error("request timed out")]
    Timeout,

    /// A successful result could not be deserialized into the expected type.
    #[error("could not decode result: {0}")]
    Decode(String),

    /// The connection could not be established or negotiated (handshake,
    /// version, capability mismatch).
    #[error("protocol error: {0}")]
    Protocol(String),
}

impl ClientError {
    /// The JSON-RPC error this call returned, if the failure was an error
    /// response (rather than a transport/timeout/decode failure).
    #[must_use]
    pub fn as_rpc(&self) -> Option<&JsonRpcError> {
        match self {
            Self::Rpc(e) => Some(e),
            Self::Http(e) => e.rpc.as_ref(),
            _ => None,
        }
    }

    /// The JSON-RPC error code, if this was an error response.
    #[must_use]
    pub fn rpc_code(&self) -> Option<i32> {
        self.as_rpc().map(|e| e.code)
    }
}
