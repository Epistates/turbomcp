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

use std::sync::Arc;

use dashmap::DashMap;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use turbomcp_types::{ClientCapabilities, ProtocolVersion};

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

pub(crate) fn request_id_key(id: &Value) -> Option<String> {
    serde_json::to_string(id).ok()
}

pub(crate) fn client_capabilities_from_initialize_params(
    params: Option<&Value>,
) -> ClientCapabilities {
    params
        .and_then(|params| params.get("capabilities"))
        .cloned()
        .and_then(|capabilities| serde_json::from_value(capabilities).ok())
        .unwrap_or_default()
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
