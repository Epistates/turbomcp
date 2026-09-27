//! [`LegacySessionAdapter`]: the per-connection session bridge for
//! single-client transports (stdio, TCP, …).
//!
//! HTTP carries the legacy session in the `Mcp-Session-Id` header, so the HTTP
//! runner does its own routing. A byte-pipe transport has no headers — the
//! *connection* is the session. This adapter wraps the dispatcher and supplies
//! what the pipe can't say in-band:
//!
//! 1. On `initialize`, a session id is minted and attached; once the inner
//!    service answers successfully, the connection is marked legacy.
//! 2. Subsequent messages that don't carry their own protocol version (a
//!    modern client states it per request) are stamped with the session's
//!    *negotiated* version, and carry its [`SessionId`]. Which version comes
//!    from the handshake — a `2025-06-18` client must not have its requests
//!    stamped `2025-11-25`, or it would be answered in a wire shape it does
//!    not know.
//!
//! The adapter is itself an `McpService`, so it slots into `serve`/
//! `serve_stdio` wherever a bare dispatcher would.
//!
//! The session id travels in the request's extensions, so a client can't name
//! someone else's session: there is nothing in the message to forge.

use std::future::poll_fn;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use futures::future::BoxFuture;
use serde_json::json;
use tokio::sync::watch;
use tower::Service;
use turbomcp_core::{JsonRpcMessage, McpRequest, ProtocolVersion, SessionId, meta};
use turbomcp_protocol::{methods, version};
use turbomcp_service::ProtocolError;
use uuid::Uuid;

/// Wraps an inner `Service<McpRequest>` (normally the
/// [`VersionDispatcher`](crate::VersionDispatcher)) with per-connection legacy
/// session tracking. Construct one adapter per connection; clones share the
/// connection's session state.
pub struct LegacySessionAdapter<S> {
    inner: S,
    /// `Some(_)` once an `initialize` on this connection succeeded.
    session: Arc<Mutex<Option<Session>>>,
    /// Set while an `initialize` is being answered; its sender drops once the
    /// outcome is committed, which is what a message pipelined behind the
    /// handshake waits on.
    handshake: Arc<Mutex<Option<watch::Receiver<()>>>>,
}

impl<S> core::fmt::Debug for LegacySessionAdapter<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LegacySessionAdapter")
            .field(
                "handshaken",
                &self.session.lock().is_ok_and(|s| s.is_some()),
            )
            .finish_non_exhaustive()
    }
}

/// What a completed handshake established for this connection.
#[derive(Clone)]
struct Session {
    id: String,
    /// The version `initialize` settled on — stamped onto every later
    /// version-less message so dispatch answers in the right wire shape.
    version: ProtocolVersion,
}

impl<S> LegacySessionAdapter<S> {
    /// Wrap `inner` with fresh (not-yet-initialized) connection state.
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            session: Arc::new(Mutex::new(None)),
            handshake: Arc::new(Mutex::new(None)),
        }
    }

    /// The handshake a message must wait behind, if one is still in flight.
    fn pending_handshake(&self) -> Option<watch::Receiver<()>> {
        let mut gate = self.handshake.lock().expect("handshake gate poisoned");
        match gate.as_ref() {
            // `Err` means the sender dropped: the handshake is settled.
            Some(rx) if rx.has_changed().is_ok() => gate.clone(),
            _ => {
                *gate = None;
                None
            }
        }
    }
}

/// Stamp a version-less message with the connection's session, if it has one:
/// the negotiated version into `_meta` (where the dispatcher reads a
/// request's version), the session id into the request's extensions.
fn stamp(session: &Mutex<Option<Session>>, request: &mut McpRequest) {
    let session = session.lock().expect("session state lock poisoned").clone();
    let Some(session) = session else { return };
    let msg = &mut request.message;
    let params = match &*msg {
        JsonRpcMessage::Request(r) => r.params.as_ref(),
        JsonRpcMessage::Notification(n) => n.params.as_ref(),
        JsonRpcMessage::Response(_) => None,
    };
    // Stamp only version-less messages: a modern stateless client sharing
    // the pipe keeps working, per-request version wins.
    if version::request_protocol_version(params).is_none() {
        meta::set_request_meta(
            msg,
            meta::keys::PROTOCOL_VERSION,
            json!(session.version.as_str()),
        );
        request.extensions.insert(SessionId::new(session.id));
    }
}

impl<S: Clone> Clone for LegacySessionAdapter<S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            session: Arc::clone(&self.session),
            handshake: Arc::clone(&self.handshake),
        }
    }
}

impl<S> Service<McpRequest> for LegacySessionAdapter<S>
where
    S: Service<McpRequest, Response = Option<JsonRpcMessage>, Error = ProtocolError>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
{
    type Response = Option<JsonRpcMessage>;
    type Error = ProtocolError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut msg: McpRequest) -> Self::Future {
        let is_initialize = matches!(
            &msg.message,
            JsonRpcMessage::Request(r) if r.method == methods::request::INITIALIZE
        );
        if is_initialize {
            // Mint the connection's session id now; commit it only once the
            // handshake actually succeeds, so a malformed initialize doesn't
            // flip the connection into legacy mode.
            let candidate = Uuid::new_v4().to_string();
            msg.extensions.insert(SessionId::new(candidate.as_str()));
            let session = Arc::clone(&self.session);
            let (done, waiters) = watch::channel(());
            *self.handshake.lock().expect("handshake gate poisoned") = Some(waiters);
            let fut = self.inner.call(msg);
            return Box::pin(async move {
                // Dropped on every way out, after the outcome is committed.
                let _done = done;
                let out = fut.await?;
                if let Some(JsonRpcMessage::Response(resp)) = &out
                    && !resp.is_error()
                {
                    // Read back what the handshake negotiated rather than
                    // assuming: the server may answer a version other than the
                    // one requested, and every later message is stamped with it.
                    let version = resp
                        .result
                        .as_ref()
                        .and_then(|r| r.get("protocolVersion"))
                        .and_then(serde_json::Value::as_str)
                        .map_or(ProtocolVersion::V2025_11_25, ProtocolVersion::from_wire);
                    *session.lock().expect("session state lock poisoned") = Some(Session {
                        id: candidate,
                        version,
                    });
                }
                Ok(out)
            });
        }

        // A message pipelined behind an `initialize` that hasn't been
        // answered yet (`printf '%s\n' "$init" "$list" | server` does exactly
        // this) waits for it, then takes the session it established. Drivers
        // call in arrival order, so reading the session now would find none.
        if let Some(mut handshake) = self.pending_handshake() {
            let session = Arc::clone(&self.session);
            let mut inner = self.inner.clone();
            return Box::pin(async move {
                let _ = handshake.changed().await; // `Err` once it settles
                stamp(&session, &mut msg);
                poll_fn(|cx| inner.poll_ready(cx)).await?;
                inner.call(msg).await
            });
        }
        stamp(&self.session, &mut msg);
        Box::pin(self.inner.call(msg))
    }
}
