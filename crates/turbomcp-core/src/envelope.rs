//! [`McpRequest`] — a message plus the typed facts that travel beside it.
//!
//! A transport knows things about a request the message itself doesn't say:
//! which connection it arrived on, which legacy session it belongs to, who
//! authenticated, which HTTP headers mirrored its arguments. Those facts ride
//! in [`McpRequest::extensions`], a type-map the client cannot write to, rather
//! than in reserved `_meta` keys every transport had to scrub from client input
//! before adding its own.
//!
//! The facts defined here are the ones the foundation can name. The service
//! layer adds its own (the outbound `Peer`, the HTTP session stream registry),
//! and middleware may add anything.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use core::fmt;

use crate::{Extensions, JsonRpcMessage};

/// A JSON-RPC message and the typed facts its transport attached.
///
/// This is the request type of the service seam: every layer between a
/// transport and the dispatcher sees one.
#[derive(Clone, Debug)]
pub struct McpRequest {
    /// The message as it arrived.
    pub message: JsonRpcMessage,
    /// What the transport (and any middleware) knows about it.
    pub extensions: Extensions,
}

impl McpRequest {
    /// A message with no facts attached (an in-process caller, a test).
    #[must_use]
    pub fn new(message: impl Into<JsonRpcMessage>) -> Self {
        Self {
            message: message.into(),
            extensions: Extensions::new(),
        }
    }

    /// Builder: attach a fact.
    #[must_use]
    pub fn with<T: core::any::Any + Send + Sync>(mut self, fact: T) -> Self {
        self.extensions.insert(fact);
        self
    }

    /// The protocol revision this request speaks, from its `_meta`.
    ///
    /// A `2026-07-28` request states it itself. A stateful one states it once,
    /// at `initialize`; by the time a request reaches middleware the runtime
    /// has stamped its session's negotiated revision in, so this answers for
    /// both. `None` for a response, or a request with no version anywhere
    /// (which the dispatcher refuses).
    ///
    /// For rendering a refusal the way that revision spells it:
    /// `McpError::to_jsonrpc_error(&version)`.
    #[must_use]
    pub fn protocol_version(&self) -> Option<crate::ProtocolVersion> {
        let params = match &self.message {
            JsonRpcMessage::Request(r) => r.params.as_ref(),
            JsonRpcMessage::Notification(n) => n.params.as_ref(),
            JsonRpcMessage::Response(_) => None,
        }?;
        crate::meta::extract_protocol_version(params.get("_meta")?.as_object()?)
    }
}

impl From<JsonRpcMessage> for McpRequest {
    fn from(message: JsonRpcMessage) -> Self {
        Self::new(message)
    }
}

impl From<crate::JsonRpcRequest> for McpRequest {
    fn from(request: crate::JsonRpcRequest) -> Self {
        Self::new(request)
    }
}

impl From<crate::JsonRpcNotification> for McpRequest {
    fn from(notification: crate::JsonRpcNotification) -> Self {
        Self::new(notification)
    }
}

impl From<crate::JsonRpcResponse> for McpRequest {
    fn from(response: crate::JsonRpcResponse) -> Self {
        Self::new(response)
    }
}

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, PartialEq, Eq, Hash)]
        pub struct $name(Arc<str>);

        impl $name {
            /// Wrap an id.
            #[must_use]
            pub fn new(id: impl Into<Arc<str>>) -> Self {
                Self(id.into())
            }

            /// The id as a string.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_tuple(stringify!($name)).field(&&*self.0).finish()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(id: String) -> Self {
                Self::new(id)
            }
        }

        impl From<&str> for $name {
            fn from(id: &str) -> Self {
                Self::new(id)
            }
        }
    };
}

id_type! {
    /// The connection a message arrived on: one per `serve` connection, or one
    /// per HTTP response stream. Scopes `notifications/cancelled` (a client can
    /// only cancel its own requests) and `subscriptions/listen` streams.
    ConnectionId
}

id_type! {
    /// The stateful (`2025-06-18` / `2025-11-25`) session a message belongs to:
    /// from the `Mcp-Session-Id` header on HTTP, or the connection's own
    /// handshake on a byte pipe.
    SessionId
}

/// The `Mcp-Param-{name}` headers that arrived with a Streamable HTTP request,
/// from the lowercased `{name}` to the raw value (`None` when the value wasn't
/// visible ASCII).
///
/// Only HTTP has headers, so only HTTP attaches this. The dispatcher, which
/// knows which argument each `x-mcp-header` annotation names, checks the
/// values against the body and treats a missing mirror as a mismatch: the
/// case where a gateway routes on one value while the server runs on another.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObservedHeaders(pub BTreeMap<String, Option<String>>);

impl ObservedHeaders {
    /// The raw value of `Mcp-Param-{name}` (`name` lowercased), if it arrived.
    /// `Some(None)` is a header whose value wasn't visible ASCII.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<Option<&str>> {
        self.0.get(name).map(Option::as_deref)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{JsonRpcRequest, JsonRpcResponse, ProtocolVersion};
    use serde_json::json;

    #[test]
    fn protocol_version_reads_the_request_meta() {
        let stamped = McpRequest::new(JsonRpcRequest::new(
            1,
            "tools/list",
            Some(json!({ "_meta": { "io.modelcontextprotocol/protocolVersion": "2025-11-25" } })),
        ));
        assert_eq!(
            stamped.protocol_version(),
            Some(ProtocolVersion::V2025_11_25)
        );
        let bare = McpRequest::new(JsonRpcRequest::new(1, "tools/list", None));
        assert_eq!(bare.protocol_version(), None);
        let response = McpRequest::new(JsonRpcResponse::success(1, json!({})));
        assert_eq!(response.protocol_version(), None);
    }
}
