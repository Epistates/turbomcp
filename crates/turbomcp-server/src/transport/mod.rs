//! v3 Transport module - shared abstractions for all transports.
//!
//! This module provides:
//! - Common constants and types
//! - Shared line-based transport runner
//! - Transport-specific implementations
//!
//! # Architecture
//!
//! All transports share a common pattern:
//! 1. Read incoming messages (line-based or frame-based)
//! 2. Parse as JSON-RPC
//! 3. Route to handler
//! 4. Send response
//!
//! The `LineTransportRunner` provides a reusable implementation for
//! line-based protocols (STDIO, TCP, Unix).

mod line;

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use dashmap::DashMap;
use futures::FutureExt;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use turbomcp_core::error::McpError;
use turbomcp_core::handler::McpHandler;
use turbomcp_types::{ClientCapabilities, ProtocolVersion};

use crate::context::RequestContext;
use crate::router::{self, JsonRpcIncoming, JsonRpcOutgoing};

/// How long a server-to-client request waits for the client's reply.
///
/// Sampling and elicitation block a handler on the peer. Without a bound the
/// handler waits forever on a client that never answers — a hung tool call and
/// a leaked task per occurrence, entirely under the peer's control. Sixty
/// seconds is long enough for a human-in-the-loop approval, which is the
/// slowest legitimate case.
pub(crate) const SERVER_REQUEST_TIMEOUT: core::time::Duration = core::time::Duration::from_secs(60);

/// Route a request, turning a handler panic into a JSON-RPC error response.
///
/// A panicking handler previously produced **no response at all**: the spawned
/// task unwound before reaching the send, so the client was left waiting on an
/// id that would never be answered. Most MCP clients have no per-request
/// timeout, which makes that wait permanent.
///
/// A panic is a server fault, so it is reported as one (`-32603`). The panic
/// payload is logged in full but only summarised to the client, since it can
/// contain internal detail.
pub(crate) async fn route_catching_panics<H: McpHandler>(
    handler: &H,
    request: JsonRpcIncoming,
    ctx: &RequestContext,
    version: &ProtocolVersion,
) -> JsonRpcOutgoing {
    let id = request.id.clone();
    let method = request.method.clone();

    match AssertUnwindSafe(router::route_request_versioned(
        handler, request, ctx, version,
    ))
    .catch_unwind()
    .await
    {
        Ok(response) => response,
        Err(payload) => {
            let detail = panic_detail(&payload);
            tracing::error!(
                method = %method,
                panic = %detail,
                "Handler panicked; answering with an internal error"
            );
            // `id` is `None` for a notification, and an error with no id is
            // suppressed by `should_send()` — correct, since notifications
            // take no reply.
            JsonRpcOutgoing::error(
                id,
                McpError::internal(format!("Handler panicked while serving {method}")),
            )
        }
    }
}

/// Best-effort readable form of a panic payload, for the server's own log.
fn panic_detail(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

/// RAII guard that removes a pending-handler entry from the per-connection
/// cancellation registry when dropped.
///
/// The line / channel / websocket transports each maintain a
/// `DashMap<request_id, CancellationToken>` so that an inbound
/// `notifications/cancelled` can signal the matching in-flight handler.
/// The handler's spawned task removes its own entry on the success path,
/// but `tokio::spawn` catches panics — without a Drop-based cleanup, a
/// panicking handler would leak its registry entry for the connection's
/// lifetime. This guard runs cleanup on all paths (success, error, panic,
/// future drop).
pub(crate) struct PendingHandlerGuard {
    handlers: Arc<DashMap<String, CancellationToken>>,
    key: Option<String>,
}

impl PendingHandlerGuard {
    pub(crate) fn new(
        handlers: Arc<DashMap<String, CancellationToken>>,
        key: Option<String>,
    ) -> Self {
        Self { handlers, key }
    }
}

impl Drop for PendingHandlerGuard {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            self.handlers.remove(&key);
        }
    }
}

/// MCP session lifecycle state for per-connection/session version tracking.
///
/// Enforces the MCP spec initialization lifecycle:
/// 1. Client sends `initialize` → server responds with negotiated version
/// 2. Client sends `notifications/initialized`
/// 3. Normal operation begins
///
/// Requests arriving before successful initialization are rejected.
/// Duplicate `initialize` requests after a successful handshake are rejected.
///
/// Request-id reuse is deliberately *not* tracked here. The spec's
/// "MUST NOT have been previously used" binds the **requestor**; a receiver's
/// only obligation is to echo the id back. Enforcing it server-side bought
/// nothing, cost an unbounded per-session set, and rejected real clients — see
/// the note on the transports' in-flight registries.
#[derive(Debug, Clone)]
pub(crate) enum SessionState {
    /// No successful `initialize` has been received yet.
    Uninitialized,
    /// `initialize` succeeded; the negotiated protocol version is stored.
    Initialized(InitializedSessionState),
}

#[derive(Debug, Clone)]
pub(crate) struct InitializedSessionState {
    protocol_version: ProtocolVersion,
}

impl InitializedSessionState {
    pub(crate) fn new(protocol_version: ProtocolVersion) -> Self {
        Self { protocol_version }
    }

    pub(crate) fn protocol_version(&self) -> &ProtocolVersion {
        &self.protocol_version
    }
}

/// Render a JSON-RPC `id` (string | number) as a stable string key so that
/// `42` from the request and `"42"` from `notifications/cancelled.requestId`
/// share a slot in the cancellation registry.
///
/// Every transport keys its in-flight registry with this one function. They
/// have to agree: a client is free to send the id as a number in the request
/// and a string in the cancellation (JSON-RPC does not constrain it), and two
/// renderings would mean the cancel silently matched nothing.
pub(crate) fn jsonrpc_id_key(id: &Value) -> String {
    match id {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

/// Read the client's declared capabilities out of the `initialize` params.
///
/// Deserialized **field by field** rather than in one shot. Capabilities are
/// independent declarations, and the spec has each side ignore what it does not
/// understand; a single unparseable sibling — an unknown sub-capability, a
/// draft extension, a peer that is simply newer — must not erase the rest.
///
/// The all-or-nothing version silently returned `ClientCapabilities::default()`
/// for the whole object, so one odd field made the server believe the client
/// had declared *nothing*, and every server-initiated call then failed with
/// `capability_not_supported`.
pub(crate) fn client_capabilities_from_initialize_params(
    params: Option<&Value>,
) -> ClientCapabilities {
    let Some(caps) = params.and_then(|params| params.get("capabilities")) else {
        return ClientCapabilities::default();
    };

    // The whole object first: the common case, and it preserves any field the
    // per-field pass below does not name.
    if let Ok(parsed) = serde_json::from_value::<ClientCapabilities>(caps.clone()) {
        return parsed;
    }

    let mut out = ClientCapabilities::default();
    let field = |name: &str| caps.get(name).cloned();
    out.roots = field("roots").and_then(|v| serde_json::from_value(v).ok());
    out.sampling = field("sampling").and_then(|v| serde_json::from_value(v).ok());
    out.elicitation = field("elicitation").and_then(|v| serde_json::from_value(v).ok());
    out.experimental = field("experimental").and_then(|v| serde_json::from_value(v).ok());
    out.tasks = field("tasks").and_then(|v| serde_json::from_value(v).ok());

    tracing::debug!(
        "client capabilities did not deserialize as a whole; recovered per field: \
         roots={} sampling={} elicitation={}",
        out.roots.is_some(),
        out.sampling.is_some(),
        out.elicitation.is_some(),
    );
    out
}

#[cfg(feature = "stdio")]
pub mod stdio;

#[cfg(feature = "tcp")]
pub mod tcp;

#[cfg(all(feature = "unix", unix))]
pub mod unix;

#[cfg(feature = "channel")]
pub mod channel;

#[cfg(feature = "http")]
pub mod http;

#[cfg(feature = "websocket")]
pub mod websocket;

pub use line::{LineReader, LineTransportRunner, LineWriter};

// Re-export the configurable default from config
pub use crate::config::DEFAULT_MAX_MESSAGE_SIZE;

/// Maximum message size for line-based transports.
/// This prevents memory exhaustion from maliciously large messages.
/// Use `ServerConfig::max_message_size` for runtime configuration.
pub const MAX_MESSAGE_SIZE: usize = DEFAULT_MAX_MESSAGE_SIZE;
