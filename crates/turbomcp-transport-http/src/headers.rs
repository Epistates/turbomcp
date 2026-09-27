//! The Streamable HTTP transport's header names, shared by both halves.
//!
//! How header *values* mirror body fields (the `Mcp-Param-*` value codec, the
//! argument rendering rules) is in
//! [`turbomcp_service::mcp_headers`], because the typed client and the
//! dispatcher use it without knowing about HTTP.

use http::HeaderName;

/// `Mcp-Session-Id`: the stateful (`2025-06-18` / `2025-11-25`) session.
pub const SESSION_ID: HeaderName = HeaderName::from_static("mcp-session-id");
/// `MCP-Protocol-Version`: required on every POST after the handshake; on
/// `2026-07-28` it must equal the body's `_meta` protocol version.
pub const PROTOCOL_VERSION: HeaderName = HeaderName::from_static("mcp-protocol-version");
/// `Mcp-Method`: required on every `2026-07-28` request POST; mirrors
/// `method`.
pub const MCP_METHOD: HeaderName = HeaderName::from_static("mcp-method");
/// `Mcp-Name`: mirrors `params.name` / `params.uri` (and a task poll's
/// `taskId`) for routing.
pub const MCP_NAME: HeaderName = HeaderName::from_static("mcp-name");
/// `Last-Event-ID`: where a resumed SSE stream picks up.
pub const LAST_EVENT_ID: HeaderName = HeaderName::from_static("last-event-id");
/// The `Mcp-Param-{name}` prefix for `x-mcp-header` tool arguments, lowercase
/// as HTTP/2 carries header names.
pub const MCP_PARAM_PREFIX: &str = "mcp-param-";
