//! Unified error type for TurboMCP v4.
//!
//! [`McpError`] is the single user-facing error type across the SDK
//! (`McpResult<T> = Result<T, McpError>`). It carries the canonical
//! `McpError → JSON-RPC code → HTTP status` mapping (PLAN.md §4.10) in one
//! place so handler authors never re-invent error policy.
//!
//! `no_std`: errors impl [`core::error::Error`] (stable since Rust 1.81), so no
//! `thiserror` dependency is needed and the type works on `wasm32`.

use alloc::string::{String, ToString};
use core::fmt;

/// The result type returned by TurboMCP handlers and most fallible APIs.
pub type McpResult<T> = Result<T, McpError>;

/// The unified error type for TurboMCP v4.
///
/// Variants map to JSON-RPC error codes and HTTP statuses via
/// [`McpError::jsonrpc_code`] and [`McpError::http_status`]. The mapping is the
/// single source of truth referenced by the dispatcher and HTTP transport.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum McpError {
    /// Unexpected internal failure. JSON-RPC `-32603`, HTTP 500.
    Internal(String),
    /// Malformed or invalid request parameters at the protocol level.
    /// JSON-RPC `-32602`, HTTP 400.
    InvalidParams(String),
    /// The requested method is not implemented. JSON-RPC `-32601`, HTTP 404.
    MethodNotFound(String),
    /// The named tool does not exist. JSON-RPC `-32601`, HTTP 404.
    ToolNotFound(String),
    /// A tool ran but failed. **Not a protocol error** — the dispatcher
    /// surfaces this as `CallToolResult { isError: true }` (HTTP 200), never as
    /// a JSON-RPC error. See PLAN.md §4.11.
    ToolExecutionFailed {
        /// Tool name.
        tool: String,
        /// Failure reason (becomes tool error content).
        reason: String,
    },
    /// The requested resource URI was not found. JSON-RPC `-32602`
    /// (Invalid Params — the 2026-07-28 RC moved it off `-32002`), HTTP 404.
    ResourceNotFound(String),
    /// Authentication failed or is required. JSON-RPC `-32000`, HTTP 401.
    Authentication(String),
    /// The identity is authenticated but not permitted. JSON-RPC `-32000`,
    /// HTTP 403.
    PermissionDenied(String),
    /// The operation timed out. JSON-RPC `-32000`, HTTP 504. Retryable.
    Timeout(String),
    /// Transport-level failure (connection closed, I/O error). JSON-RPC
    /// `-32000`, HTTP 503. Retryable.
    Transport(String),
    /// The requested protocol version is not supported. JSON-RPC `-32004`,
    /// HTTP 400. Carries the requested version (or `None` when absent).
    UnsupportedProtocolVersion(String),
    /// The request requires a client capability that was not advertised.
    /// JSON-RPC `-32003`, HTTP 400.
    MissingRequiredCapability(String),
    /// An HTTP header did not match the corresponding request-body value
    /// (`MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name`, `Mcp-Param-*`).
    /// JSON-RPC `-32001`, HTTP 400.
    HeaderMismatch(String),
    /// MRTR abort sentinel (SEP-2322): a handler asked the client for input
    /// (`ctx.client.elicit(…)`) that the request didn't carry yet. It exists
    /// so the abort can ride `?` through user code; the dispatcher intercepts
    /// it and answers an `InputRequiredResult` — it is never a user-visible
    /// error. The codes below are defensive fallbacks only.
    #[doc(hidden)]
    InputRequired,
}

impl McpError {
    /// Construct an [`McpError::Internal`].
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }
    /// Construct an [`McpError::InvalidParams`].
    pub fn invalid_params(msg: impl Into<String>) -> Self {
        Self::InvalidParams(msg.into())
    }
    /// Construct an [`McpError::MethodNotFound`].
    pub fn method_not_found(msg: impl Into<String>) -> Self {
        Self::MethodNotFound(msg.into())
    }
    /// Construct an [`McpError::ToolNotFound`].
    pub fn tool_not_found(name: impl Into<String>) -> Self {
        Self::ToolNotFound(name.into())
    }
    /// Construct an [`McpError::ToolExecutionFailed`].
    pub fn tool_execution_failed(tool: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::ToolExecutionFailed {
            tool: tool.into(),
            reason: reason.into(),
        }
    }
    /// Construct an [`McpError::ResourceNotFound`].
    pub fn resource_not_found(uri: impl Into<String>) -> Self {
        Self::ResourceNotFound(uri.into())
    }
    /// Construct an [`McpError::Authentication`].
    pub fn authentication(msg: impl Into<String>) -> Self {
        Self::Authentication(msg.into())
    }
    /// Construct an [`McpError::PermissionDenied`].
    pub fn permission_denied(msg: impl Into<String>) -> Self {
        Self::PermissionDenied(msg.into())
    }
    /// Construct an [`McpError::Timeout`].
    pub fn timeout(msg: impl Into<String>) -> Self {
        Self::Timeout(msg.into())
    }
    /// Construct an [`McpError::Transport`].
    pub fn transport(msg: impl Into<String>) -> Self {
        Self::Transport(msg.into())
    }

    /// The JSON-RPC error code for this variant (PLAN.md §4.10).
    ///
    /// Note: [`McpError::ToolExecutionFailed`] has no protocol-level code — the
    /// dispatcher must surface it as `CallToolResult { isError: true }` and not
    /// call this. We return `-32603` as a defensive fallback only.
    #[must_use]
    pub fn jsonrpc_code(&self) -> i32 {
        match self {
            Self::Internal(_) | Self::ToolExecutionFailed { .. } | Self::InputRequired => -32603,
            Self::InvalidParams(_) | Self::ResourceNotFound(_) => -32602,
            Self::MethodNotFound(_) | Self::ToolNotFound(_) => -32601,
            // `-32000` is the implementation-defined server-error floor;
            // `-32001..` upward is claimed by the spec's allocations below.
            Self::Authentication(_)
            | Self::PermissionDenied(_)
            | Self::Timeout(_)
            | Self::Transport(_) => -32000,
            // Spec-allocated codes (2026-07-28 RC error-code allocation).
            Self::HeaderMismatch(_) => -32001,
            Self::MissingRequiredCapability(_) => -32003,
            Self::UnsupportedProtocolVersion(_) => -32004,
        }
    }

    /// The JSON-RPC error code for this variant **as `version` spells it**.
    ///
    /// One code is version-split: resource-not-found is `-32002` through
    /// `2025-11-25` and `-32602` (Invalid Params) from the `2026-07-28` RC on,
    /// which renumbered it to align with JSON-RPC. Everything else matches
    /// [`jsonrpc_code`](Self::jsonrpc_code), which answers for the current
    /// revision — prefer this method wherever the negotiated version is known.
    #[must_use]
    pub fn jsonrpc_code_for(&self, version: &crate::ProtocolVersion) -> i32 {
        use crate::ProtocolVersion as V;
        match self {
            Self::ResourceNotFound(_)
                if matches!(
                    version,
                    V::V2024_11_05 | V::V2025_03_26 | V::V2025_06_18 | V::V2025_11_25
                ) =>
            {
                -32002
            }
            other => other.jsonrpc_code(),
        }
    }

    /// The HTTP status equivalent for this variant (PLAN.md §4.10).
    ///
    /// An embedder affordance: the bundled Streamable HTTP transport never
    /// calls this — a handler's `McpError` renders as a JSON-RPC error *body*
    /// over HTTP 200, per spec. Use it when surfacing `McpError` through your
    /// own HTTP layer (REST gateways, health/admin endpoints, custom
    /// authenticators).
    #[must_use]
    pub fn http_status(&self) -> u16 {
        match self {
            Self::Internal(_) | Self::InputRequired => 500,
            Self::InvalidParams(_)
            | Self::UnsupportedProtocolVersion(_)
            | Self::MissingRequiredCapability(_)
            | Self::HeaderMismatch(_) => 400,
            Self::MethodNotFound(_) | Self::ToolNotFound(_) | Self::ResourceNotFound(_) => 404,
            Self::ToolExecutionFailed { .. } => 200,
            Self::Authentication(_) => 401,
            Self::PermissionDenied(_) => 403,
            Self::Timeout(_) => 504,
            Self::Transport(_) => 503,
        }
    }

    /// Whether a caller may reasonably retry after this error.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Timeout(_) | Self::Transport(_))
    }
}

impl fmt::Display for McpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Internal(m) => write!(f, "internal error: {m}"),
            Self::InvalidParams(m) => write!(f, "invalid params: {m}"),
            Self::MethodNotFound(m) => write!(f, "method not found: {m}"),
            Self::ToolNotFound(m) => write!(f, "tool not found: {m}"),
            Self::ToolExecutionFailed { tool, reason } => {
                write!(f, "tool '{tool}' failed: {reason}")
            }
            Self::ResourceNotFound(m) => write!(f, "resource not found: {m}"),
            Self::Authentication(m) => write!(f, "authentication error: {m}"),
            Self::PermissionDenied(m) => write!(f, "permission denied: {m}"),
            Self::Timeout(m) => write!(f, "timeout: {m}"),
            Self::Transport(m) => write!(f, "transport error: {m}"),
            Self::UnsupportedProtocolVersion(m) => {
                write!(f, "unsupported protocol version: {m}")
            }
            Self::MissingRequiredCapability(m) => {
                write!(f, "missing required capability: {m}")
            }
            Self::HeaderMismatch(m) => write!(f, "header mismatch: {m}"),
            Self::InputRequired => write!(f, "input required (unintercepted MRTR abort)"),
        }
    }
}

impl core::error::Error for McpError {}

impl From<serde_json::Error> for McpError {
    fn from(e: serde_json::Error) -> Self {
        Self::InvalidParams(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_and_status_mapping() {
        assert_eq!(McpError::internal("x").jsonrpc_code(), -32603);
        assert_eq!(McpError::invalid_params("x").jsonrpc_code(), -32602);
        assert_eq!(McpError::method_not_found("x").jsonrpc_code(), -32601);
        assert_eq!(
            McpError::UnsupportedProtocolVersion("x".into()).jsonrpc_code(),
            -32004
        );
        assert_eq!(
            McpError::MissingRequiredCapability("x".into()).jsonrpc_code(),
            -32003
        );
        assert_eq!(McpError::HeaderMismatch("x".into()).jsonrpc_code(), -32001);
        assert_eq!(McpError::authentication("x").http_status(), 401);
        assert_eq!(McpError::permission_denied("x").http_status(), 403);
        assert_eq!(McpError::timeout("x").http_status(), 504);
        // Tool execution failure is a tool-level error, not protocol-level.
        assert_eq!(
            McpError::tool_execution_failed("t", "boom").http_status(),
            200
        );
    }

    #[test]
    fn resource_not_found_is_version_split() {
        use crate::ProtocolVersion as V;
        let err = McpError::resource_not_found("mem://gone");
        // The RC renumbered it to Invalid Params; earlier revisions keep the
        // MCP-specific -32002 their spec text prescribes.
        assert_eq!(err.jsonrpc_code(), -32602, "version-agnostic = current");
        assert_eq!(err.jsonrpc_code_for(&V::Draft), -32602);
        for legacy in [
            V::V2024_11_05,
            V::V2025_03_26,
            V::V2025_06_18,
            V::V2025_11_25,
        ] {
            assert_eq!(err.jsonrpc_code_for(&legacy), -32002, "{legacy:?}");
        }
        // Every other variant is version-stable.
        for e in [
            McpError::internal("x"),
            McpError::invalid_params("x"),
            McpError::tool_not_found("x"),
            McpError::HeaderMismatch("x".into()),
        ] {
            assert_eq!(e.jsonrpc_code_for(&V::V2025_11_25), e.jsonrpc_code(), "{e}");
        }
    }

    #[test]
    fn auth_group_maps_to_minus_32000() {
        // `-32000` (implementation-defined floor) — NOT `-32001`, which the
        // 2026-07-28 RC allocates to `HeaderMismatch`.
        for e in [
            McpError::authentication("x"),
            McpError::permission_denied("x"),
            McpError::timeout("x"),
            McpError::transport("x"),
        ] {
            assert_eq!(e.jsonrpc_code(), -32000, "{e}");
        }
    }

    #[test]
    fn http_status_covers_every_variant() {
        // The full PLAN.md §4.10 table — a regression in any arm must fail here.
        assert_eq!(McpError::internal("x").http_status(), 500);
        assert_eq!(McpError::InputRequired.http_status(), 500);
        assert_eq!(McpError::invalid_params("x").http_status(), 400);
        assert_eq!(
            McpError::UnsupportedProtocolVersion("x".into()).http_status(),
            400
        );
        assert_eq!(
            McpError::MissingRequiredCapability("x".into()).http_status(),
            400
        );
        assert_eq!(McpError::HeaderMismatch("x".into()).http_status(), 400);
        assert_eq!(McpError::method_not_found("x").http_status(), 404);
        assert_eq!(McpError::tool_not_found("x").http_status(), 404);
        assert_eq!(McpError::resource_not_found("x").http_status(), 404);
        assert_eq!(McpError::authentication("x").http_status(), 401);
        assert_eq!(McpError::permission_denied("x").http_status(), 403);
        assert_eq!(McpError::timeout("x").http_status(), 504);
        assert_eq!(McpError::transport("x").http_status(), 503);
        assert_eq!(
            McpError::tool_execution_failed("t", "boom").http_status(),
            200
        );
    }

    #[test]
    fn serde_json_errors_become_invalid_params() {
        let e = serde_json::from_str::<u32>("not json").unwrap_err();
        let mcp: McpError = e.into();
        assert!(matches!(mcp, McpError::InvalidParams(_)));
        assert_eq!(mcp.jsonrpc_code(), -32602);
    }

    #[test]
    fn retryable_classification() {
        assert!(McpError::timeout("x").is_retryable());
        assert!(McpError::transport("x").is_retryable());
        assert!(!McpError::internal("x").is_retryable());
    }
}
