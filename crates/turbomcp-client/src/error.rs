//! Client-side error model (`ClientCallError` in PLAN §4.10).
//!
//! A client call fails in one of a few visible ways: the peer answered with a
//! JSON-RPC error, the connection went away, the call timed out, or a
//! successful result didn't deserialize into the type the typed API expected.
//! These are distinct enough to branch on, so they're separate variants rather
//! than one stringly error.

use turbomcp_core::{JsonRpcError, McpError, ProtocolVersion};

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
    /// The caller cancelled the call (`CallOptions::cancel_on`). The server
    /// was told to stop.
    #[error("request cancelled")]
    Cancelled,

    /// The response stream ended before the response arrived (a proxy idle
    /// timeout, a load balancer drain). The request may or may not have run;
    /// reads and lists are re-issued once automatically, `tools/call` is not.
    #[error("the response stream closed before the response arrived")]
    StreamLost,

    /// A successful result could not be deserialized into the expected type.
    #[error("could not decode result: {0}")]
    Decode(String),

    /// The connection could not be established or negotiated (handshake,
    /// version, capability mismatch).
    #[error("protocol error: {0}")]
    Protocol(String),

    /// A tool's successful result broke its own declared `outputSchema`
    /// (or omitted the `structuredContent` the schema promises).
    #[error("tool `{tool}` returned a result that violates its outputSchema: {reason}")]
    OutputSchema {
        /// The tool that was called.
        tool: String,
        /// What the validator objected to.
        reason: String,
    },
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

    /// This failure as an [`McpError`], for a server relaying a call it made
    /// to another server. `version` is the revision the *other* server
    /// speaks ([`Client::protocol_version`](crate::Client::protocol_version)).
    ///
    /// An error response comes back through
    /// [`McpError::from_jsonrpc`]: unchanged, except that the codes that
    /// differ by revision are named, so relaying one to a client on another
    /// revision sends that revision's code. Timeouts, dead connections and
    /// HTTP auth refusals become their `McpError` kinds; anything else
    /// (a result that wouldn't decode, a local refusal) is internal.
    #[must_use]
    pub fn to_mcp_error(&self, version: &ProtocolVersion) -> McpError {
        if let Some(rpc) = self.as_rpc() {
            return McpError::from_jsonrpc(rpc, version);
        }
        match self {
            Self::Http(failure) => match failure.status {
                401 => McpError::authentication(failure.to_string()),
                403 => McpError::permission_denied(failure.to_string()),
                _ => McpError::transport(failure.to_string()),
            },
            Self::Timeout => McpError::timeout(self.to_string()),
            Self::Closed | Self::StreamLost => McpError::transport(self.to_string()),
            _ => McpError::internal(self.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp_core::codes;

    /// A gateway relaying a `2026-07-28` server's refusal to a `2025-11-25`
    /// client sends that client's code for it.
    #[test]
    fn an_error_response_relays_with_the_callers_revision() {
        let upstream = ClientError::Rpc(
            McpError::resource_not_found("mem://gone")
                .to_jsonrpc_error(&ProtocolVersion::V2026_07_28),
        );
        let relayed = upstream.to_mcp_error(&ProtocolVersion::V2026_07_28);
        assert_eq!(relayed, McpError::resource_not_found("mem://gone"));
        assert_eq!(
            relayed.jsonrpc_code(&ProtocolVersion::V2025_11_25),
            codes::LEGACY_RESOURCE_NOT_FOUND
        );
    }

    #[test]
    fn failures_without_a_response_become_their_kinds() {
        let v = ProtocolVersion::V2026_07_28;
        assert!(matches!(
            ClientError::Timeout.to_mcp_error(&v),
            McpError::Timeout(_)
        ));
        assert!(matches!(
            ClientError::Closed.to_mcp_error(&v),
            McpError::Transport(_)
        ));
        let unauthorized = ClientError::Http(Box::new(turbomcp_service::HttpFailure {
            status: 401,
            message: "unauthorized".into(),
            rpc: None,
            www_authenticate: Some("Bearer".into()),
            retry_after: None,
        }));
        assert!(matches!(
            unauthorized.to_mcp_error(&v),
            McpError::Authentication(_)
        ));
        assert!(matches!(
            ClientError::Decode("x".into()).to_mcp_error(&v),
            McpError::Internal(_)
        ));
    }
}
