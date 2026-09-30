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

/// The JSON-RPC error codes: JSON-RPC 2.0's own, and the ones the MCP spec
/// allocates by name.
///
/// One definition each: these numbers appear on the wire from the dispatcher,
/// the HTTP transport, and [`McpError::jsonrpc_code`], and they have already
/// been reallocated once (the `2026-07-28` RC used `-32003`/`-32004`; the
/// frozen spec moved the block to `-3202x`). Naming them keeps the next
/// reallocation a one-line change instead of a grep.
///
/// Three groups: JSON-RPC 2.0's own; the ones the MCP spec allocates from
/// `-32020` up (the `2026-07-28` three, plus two from earlier revisions that
/// stay reserved and are never reused); and this SDK's own, on the
/// `-32000..-32019` floor the spec leaves to implementations, where a
/// receiver must not read cross-implementation meaning into them.
///
/// `just test` refuses a numeric code literal outside this module.
pub mod codes {
    /// JSON-RPC 2.0: the bytes were not valid JSON.
    pub const PARSE_ERROR: i32 = -32700;
    /// JSON-RPC 2.0: valid JSON, but not a valid request object.
    pub const INVALID_REQUEST: i32 = -32600;
    /// JSON-RPC 2.0: the method does not exist or is not available.
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// JSON-RPC 2.0: invalid method parameters.
    pub const INVALID_PARAMS: i32 = -32602;
    /// JSON-RPC 2.0: internal error.
    pub const INTERNAL_ERROR: i32 = -32603;

    /// An HTTP header disagreed with the request body, or a required header is
    /// missing or malformed (transports spec §Server Validation). HTTP 400.
    pub const HEADER_MISMATCH: i32 = -32020;
    /// The request needs a client capability that was never advertised
    /// (SEP-2322 / SEP-2663). HTTP 400.
    pub const MISSING_REQUIRED_CLIENT_CAPABILITY: i32 = -32021;
    /// The requested protocol version is not supported; `data` carries
    /// `{ supported, requested }`. HTTP 400.
    pub const UNSUPPORTED_PROTOCOL_VERSION: i32 = -32022;

    /// Resource not found, as `2025-11-25` and earlier spell it (`data`
    /// carries `{ uri }`). `2026-07-28` answers `-32602` instead and keeps
    /// this number reserved.
    pub const LEGACY_RESOURCE_NOT_FOUND: i32 = -32002;
    /// The server needs the user to visit a URL first (`2025-11-25` only;
    /// `data` carries `{ elicitations }`). Later revisions elicit in-band.
    pub const URL_ELICITATION_REQUIRED: i32 = -32042;

    /// The implementation-defined floor: authentication, authorization,
    /// timeouts, transport failures, rate limiting, and anything else with no
    /// allocated number. The HTTP status, where there is one, is the signal.
    pub const SERVER_ERROR: i32 = -32000;
    /// A task was cancelled before it finished; the outcome its result
    /// request reports.
    pub const TASK_CANCELLED: i32 = -32010;

    /// A stateful (`2025-11-25`, `2025-06-18`) request arrived with no live
    /// session: none was ever opened, or the one it named is gone. HTTP 400 /
    /// 404 depending on which.
    ///
    /// **Not** `-32002`, which those same revisions allocate to
    /// resource-not-found — sharing the number made a dead session and a
    /// missing resource indistinguishable to any client that maps codes. This
    /// condition has no allocated number, so it sits on the implementation-
    /// defined floor, where the HTTP status is the load-bearing signal anyway
    /// (the spec's session rules are written in terms of `404`).
    pub const NO_ACTIVE_SESSION: i32 = -32000;
}

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
    /// The requested protocol version is not supported. JSON-RPC `-32022`,
    /// HTTP 400. Carries the requested version (or `None` when absent).
    UnsupportedProtocolVersion(String),
    /// The request requires a client capability that was not advertised.
    /// JSON-RPC `-32021`, HTTP 400.
    MissingRequiredCapability(String),
    /// An HTTP header did not match the corresponding request-body value
    /// (`MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name`, `Mcp-Param-*`).
    /// JSON-RPC `-32020`, HTTP 400.
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

    /// The JSON-RPC error code for this error **as `version` spells it**.
    ///
    /// There is no version-less form: two codes differ by revision, and a
    /// default would be right for one and wrong for the other.
    ///
    /// - Resource not found is `-32002` through `2025-11-25` and `-32602`
    ///   (Invalid Params) from `2026-07-28`, which renumbered it to align with
    ///   JSON-RPC.
    /// - A missing client capability is `-32021` only from `2026-07-28`, which
    ///   allocated it. No older schema defines that code, and on the stateful
    ///   revisions capabilities are fixed at `initialize`, so its "re-declare
    ///   and retry" affordance cannot be used either: there it is `-32602`.
    ///
    /// [`McpError::ToolExecutionFailed`] has no protocol-level code: a tool
    /// failure is a `CallToolResult { isError: true }`. It answers `-32603`
    /// only as a fallback.
    #[must_use]
    pub fn jsonrpc_code(&self, version: &crate::ProtocolVersion) -> i32 {
        use crate::ProtocolVersion as V;
        // The revisions before `2026-07-28` renumbered. An unknown version
        // gets the current codes.
        let earlier = matches!(
            version,
            V::V2024_11_05 | V::V2025_03_26 | V::V2025_06_18 | V::V2025_11_25
        );
        match self {
            Self::Internal(_) | Self::ToolExecutionFailed { .. } | Self::InputRequired => {
                codes::INTERNAL_ERROR
            }
            Self::ResourceNotFound(_) if earlier => codes::LEGACY_RESOURCE_NOT_FOUND,
            Self::MissingRequiredCapability(_) if earlier => codes::INVALID_PARAMS,
            Self::InvalidParams(_) | Self::ResourceNotFound(_) => codes::INVALID_PARAMS,
            Self::MethodNotFound(_) | Self::ToolNotFound(_) => codes::METHOD_NOT_FOUND,
            Self::Authentication(_)
            | Self::PermissionDenied(_)
            | Self::Timeout(_)
            | Self::Transport(_) => codes::SERVER_ERROR,
            Self::HeaderMismatch(_) => codes::HEADER_MISMATCH,
            Self::MissingRequiredCapability(_) => codes::MISSING_REQUIRED_CLIENT_CAPABILITY,
            Self::UnsupportedProtocolVersion(_) => codes::UNSUPPORTED_PROTOCOL_VERSION,
        }
    }

    /// This error as the JSON-RPC error object `version` expects: its code
    /// ([`jsonrpc_code`](Self::jsonrpc_code)), its message, and the `data` the
    /// spec requires for the codes that carry one.
    ///
    /// The one place an `McpError` becomes a wire error. Middleware that
    /// refuses a request itself should answer with this, reading the version
    /// from `McpRequest::protocol_version`, so its refusal matches what the
    /// dispatcher would have said.
    #[must_use]
    pub fn to_jsonrpc_error(&self, version: &crate::ProtocolVersion) -> crate::JsonRpcError {
        crate::JsonRpcError {
            code: self.jsonrpc_code(version),
            message: self.to_string(),
            data: self.data(),
        }
    }

    /// The spec-mandated `error.data` for the errors that carry one.
    fn data(&self) -> Option<serde_json::Value> {
        use serde_json::json;
        match self {
            Self::ResourceNotFound(uri) => Some(json!({ "uri": uri })),
            // `MissingRequiredClientCapabilityError` names what the client
            // failed to declare as a `ClientCapabilities` object it can merge
            // into its own declaration and retry. The carried string may be a
            // dotted sub-capability path (`elicitation.url`); emitted flat, the
            // client would re-declare a bogus top-level key, fail the same
            // check, and retry forever. Fold the path into the nesting the type
            // actually has.
            Self::MissingRequiredCapability(capability) => {
                let required = capability
                    .split('.')
                    .rev()
                    .fold(json!({}), |acc, segment| json!({ segment: acc }));
                Some(json!({ "requiredCapabilities": required }))
            }
            _ => None,
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
        assert_eq!(
            McpError::internal("x").jsonrpc_code(&crate::ProtocolVersion::LATEST),
            -32603
        );
        assert_eq!(
            McpError::invalid_params("x").jsonrpc_code(&crate::ProtocolVersion::LATEST),
            -32602
        );
        assert_eq!(
            McpError::method_not_found("x").jsonrpc_code(&crate::ProtocolVersion::LATEST),
            -32601
        );
        // The frozen `2026-07-28` allocation; the RC used -32004/-32003 and
        // had no header-mismatch code at all.
        assert_eq!(
            McpError::UnsupportedProtocolVersion("x".into())
                .jsonrpc_code(&crate::ProtocolVersion::LATEST),
            -32022
        );
        assert_eq!(
            McpError::MissingRequiredCapability("x".into())
                .jsonrpc_code(&crate::ProtocolVersion::LATEST),
            -32021
        );
        // `-32021` is a `2026-07-28` allocation; no older schema defines it.
        let missing = McpError::MissingRequiredCapability("x".into());
        assert_eq!(
            missing.jsonrpc_code(&crate::ProtocolVersion::V2026_07_28),
            -32021
        );
        assert_eq!(
            missing.jsonrpc_code(&crate::ProtocolVersion::V2025_11_25),
            -32602
        );
        assert_eq!(
            missing.jsonrpc_code(&crate::ProtocolVersion::V2025_06_18),
            -32602
        );
        assert_eq!(
            McpError::HeaderMismatch("x".into()).jsonrpc_code(&crate::ProtocolVersion::LATEST),
            -32020
        );
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
        assert_eq!(
            err.jsonrpc_code(&V::Unknown("2099-01-01".into())),
            -32602,
            "an unknown version gets the current code"
        );
        assert_eq!(err.jsonrpc_code(&V::V2026_07_28), -32602);
        for legacy in [
            V::V2024_11_05,
            V::V2025_03_26,
            V::V2025_06_18,
            V::V2025_11_25,
        ] {
            assert_eq!(err.jsonrpc_code(&legacy), -32002, "{legacy:?}");
        }
        // Every other variant is version-stable.
        for e in [
            McpError::internal("x"),
            McpError::invalid_params("x"),
            McpError::tool_not_found("x"),
            McpError::HeaderMismatch("x".into()),
        ] {
            assert_eq!(
                e.jsonrpc_code(&V::V2025_11_25),
                e.jsonrpc_code(&crate::ProtocolVersion::LATEST),
                "{e}"
            );
        }
    }

    #[test]
    fn auth_group_maps_to_minus_32000() {
        // `-32000` (implementation-defined floor). The spec's own allocations
        // live up at `-3202x`, so this whole group stays clear of them.
        for e in [
            McpError::authentication("x"),
            McpError::permission_denied("x"),
            McpError::timeout("x"),
            McpError::transport("x"),
        ] {
            assert_eq!(
                e.jsonrpc_code(&crate::ProtocolVersion::LATEST),
                -32000,
                "{e}"
            );
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
        assert_eq!(mcp.jsonrpc_code(&crate::ProtocolVersion::LATEST), -32602);
    }

    /// A dotted sub-capability path comes back as the nesting
    /// `ClientCapabilities` actually has. `{"elicitation.url": {}}` names no
    /// field; a client that merges it and retries declares a bogus top-level
    /// key, fails the same check, and loops.
    #[test]
    fn a_dotted_capability_path_nests() {
        let v = crate::ProtocolVersion::V2026_07_28;
        let flat = McpError::MissingRequiredCapability("sampling".into()).to_jsonrpc_error(&v);
        assert_eq!(flat.code, codes::MISSING_REQUIRED_CLIENT_CAPABILITY);
        assert_eq!(
            flat.data,
            Some(serde_json::json!({ "requiredCapabilities": { "sampling": {} } }))
        );
        let nested =
            McpError::MissingRequiredCapability("elicitation.url".into()).to_jsonrpc_error(&v);
        assert_eq!(
            nested.data,
            Some(serde_json::json!({
                "requiredCapabilities": { "elicitation": { "url": {} } }
            }))
        );
    }

    #[test]
    fn resource_not_found_carries_its_uri_on_every_revision() {
        for v in crate::ProtocolVersion::SUPPORTED {
            let err = McpError::resource_not_found("mem://gone").to_jsonrpc_error(v);
            assert_eq!(
                err.data,
                Some(serde_json::json!({ "uri": "mem://gone" })),
                "{v:?}"
            );
            assert_eq!(err.message, "resource not found: mem://gone");
        }
    }

    #[test]
    fn retryable_classification() {
        assert!(McpError::timeout("x").is_retryable());
        assert!(McpError::transport("x").is_retryable());
        assert!(!McpError::internal("x").is_retryable());
    }
}
