//! The per-connection session shared by the line (STDIO, TCP, Unix),
//! WebSocket and channel transports.
//!
//! Each of those transports owns one long-lived connection and one loop that
//! is the only writer to it. Handlers talk to the client through a
//! [`ConnectionSession`], which queues [`SessionCommand`]s for that loop; the
//! loop tracks the requests it sends in [`OutboundRequests`].
//!
//! All three transports used to carry their own copy of this, and a fix to
//! one — the pending-request leak below is the case in point — never reached
//! the others.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use dashmap::DashMap;
use serde_json::Value;
use tokio::sync::{RwLock, mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use turbomcp_core::error::{ErrorKind, McpError, McpResult};
use turbomcp_types::{ClientCapabilities, ProtocolVersion};

use crate::context::{McpSession, SessionFuture};

/// Maximum number of in-flight server-to-client requests per connection.
const MAX_PENDING_REQUESTS: usize = 64;

/// Queue depth between handlers and the connection loop.
const COMMAND_BUFFER: usize = 32;

/// How long a closing connection waits for in-flight handlers to finish.
///
/// Once the client has gone, responses have nowhere useful to go, but a
/// handler midway through a side effect should be allowed to finish it. The
/// bound keeps a handler that never finishes from pinning the connection —
/// and, on TCP and Unix, its connection-limit slot — forever.
pub(crate) const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(10);

/// A message a handler asks the connection loop to send to the client.
#[derive(Debug)]
pub(crate) enum SessionCommand {
    /// A request; the loop reports the client's answer on `response_tx`.
    Request {
        id: Value,
        method: String,
        params: Value,
        response_tx: oneshot::Sender<McpResult<Value>>,
    },
    /// A notification.
    Notify { method: String, params: Value },
}

/// The handle handlers hold to reach the client of one connection.
#[derive(Debug, Clone)]
pub(crate) struct ConnectionSession {
    commands: mpsc::Sender<SessionCommand>,
    client_capabilities: Arc<RwLock<Option<ClientCapabilities>>>,
    protocol_version: Arc<RwLock<Option<ProtocolVersion>>>,
    next_request_id: Arc<AtomicU64>,
    session_id: Arc<str>,
}

impl ConnectionSession {
    /// A new session and the receiving end its connection loop drains.
    pub(crate) fn new() -> (Self, mpsc::Receiver<SessionCommand>) {
        let (commands, receiver) = mpsc::channel(COMMAND_BUFFER);
        let session = Self {
            commands,
            client_capabilities: Arc::new(RwLock::new(None)),
            protocol_version: Arc::new(RwLock::new(None)),
            next_request_id: Arc::new(AtomicU64::new(1)),
            session_id: uuid::Uuid::new_v4().to_string().into(),
        };
        (session, receiver)
    }

    /// Identifies this connection in per-session state.
    ///
    /// A connection is a session on these transports. Without an id,
    /// everything keyed by session — the `logging/setLevel` threshold, the log
    /// rate limit, per-session visibility — silently did nothing here, while
    /// working over HTTP.
    pub(crate) fn id(&self) -> &str {
        &self.session_id
    }

    /// Record the outcome of a successful `initialize`.
    pub(crate) async fn set_initialized(
        &self,
        client_capabilities: ClientCapabilities,
        protocol_version: ProtocolVersion,
    ) {
        *self.client_capabilities.write().await = Some(client_capabilities);
        *self.protocol_version.write().await = Some(protocol_version);
    }
}

impl McpSession for ConnectionSession {
    fn client_capabilities<'a>(&'a self) -> SessionFuture<'a, Option<ClientCapabilities>> {
        Box::pin(async move { Ok(self.client_capabilities.read().await.clone()) })
    }

    fn protocol_version<'a>(&'a self) -> SessionFuture<'a, Option<ProtocolVersion>> {
        Box::pin(async move { Ok(self.protocol_version.read().await.clone()) })
    }

    fn call<'a>(&'a self, method: &'a str, params: Value) -> SessionFuture<'a, Value> {
        Box::pin(async move {
            // The id is minted here rather than by the loop so that this
            // future can name the request if it has to cancel it. Prefixed so
            // it can never collide with a client's integer ids.
            let id = Value::String(format!(
                "s-{}",
                self.next_request_id.fetch_add(1, Ordering::Relaxed)
            ));
            let (response_tx, response_rx) = oneshot::channel();
            self.commands
                .send(SessionCommand::Request {
                    id: id.clone(),
                    method: method.to_string(),
                    params,
                    response_tx,
                })
                .await
                .map_err(|_| McpError::internal("Session closed"))?;

            // Armed until the client answers. If this future times out or is
            // dropped — the handler was cancelled — the client is told to stop
            // working on the request, as the timeout and cancellation
            // utilities say a sender SHOULD.
            let mut abandon = CancelOnAbandon {
                commands: self.commands.clone(),
                id: Some(id),
                reason: "server abandoned the request",
            };

            // Bounded: an unanswered server-to-client request would otherwise
            // park the handler forever, which is a hung tool call and a leaked
            // task per occurrence, entirely at the peer's discretion.
            match tokio::time::timeout(super::SERVER_REQUEST_TIMEOUT, response_rx).await {
                Ok(Ok(result)) => {
                    abandon.disarm();
                    result
                }
                Ok(Err(_)) => {
                    // The loop dropped the request: the connection is gone.
                    abandon.disarm();
                    Err(McpError::internal("Response channel closed"))
                }
                Err(_) => {
                    abandon.reason = "server request timed out";
                    Err(McpError::timeout(format!(
                        "client did not answer {method} within {:?}",
                        super::SERVER_REQUEST_TIMEOUT
                    )))
                }
            }
        })
    }

    fn notify<'a>(&'a self, method: &'a str, params: Value) -> SessionFuture<'a, ()> {
        Box::pin(async move {
            self.commands
                .send(SessionCommand::Notify {
                    method: method.to_string(),
                    params,
                })
                .await
                .map_err(|_| McpError::internal("Session closed"))?;
            Ok(())
        })
    }
}

/// Sends `notifications/cancelled` for a request nobody is waiting on any
/// more, unless disarmed first.
///
/// `try_send`, because this runs in `Drop`. A full queue or a closed
/// connection loses the notification, which is acceptable: it is advisory,
/// and the loop reclaims the request's slot either way (see
/// [`OutboundRequests::begin`]).
struct CancelOnAbandon {
    commands: mpsc::Sender<SessionCommand>,
    id: Option<Value>,
    reason: &'static str,
}

impl CancelOnAbandon {
    fn disarm(&mut self) {
        self.id = None;
    }
}

impl Drop for CancelOnAbandon {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            let _ = self.commands.try_send(SessionCommand::Notify {
                method: "notifications/cancelled".to_string(),
                params: serde_json::json!({ "requestId": id, "reason": self.reason }),
            });
        }
    }
}

/// The requests a connection loop has sent to its client and not yet seen
/// answered.
#[derive(Debug, Default)]
pub(crate) struct OutboundRequests {
    pending: HashMap<Value, oneshot::Sender<McpResult<Value>>>,
}

impl OutboundRequests {
    /// Turn a handler's command into the frame to write, registering it if it
    /// is a request. `None` means the request was refused, and its handler
    /// has already been told why.
    pub(crate) fn frame(&mut self, command: SessionCommand) -> Option<Value> {
        match command {
            SessionCommand::Request {
                id,
                method,
                params,
                response_tx,
            } => self.begin(id, &method, params, response_tx),
            SessionCommand::Notify { method, params } => Some(notification_frame(&method, params)),
        }
    }

    fn begin(
        &mut self,
        id: Value,
        method: &str,
        params: Value,
        response_tx: oneshot::Sender<McpResult<Value>>,
    ) -> Option<Value> {
        // A closed sender is a request whose handler stopped waiting — it
        // timed out, or was cancelled and dropped. Entries used to be removed
        // only when the client answered, so every unanswered request held its
        // slot for the life of the connection, and after 64 of them the
        // connection could never sample or elicit again.
        self.pending.retain(|_, waiting| !waiting.is_closed());

        if self.pending.len() >= MAX_PENDING_REQUESTS {
            tracing::error!(
                count = self.pending.len(),
                "Too many pending server-to-client requests"
            );
            let _ = response_tx.send(Err(McpError::internal(
                "Too many pending server-to-client requests",
            )));
            return None;
        }

        let frame = request_frame(&id, method, params);
        self.pending.insert(id, response_tx);
        Some(frame)
    }

    /// If `message` answers one of this loop's requests, hand the answer to
    /// the waiting handler and return `true`. Any message carrying `result`
    /// or `error` is a response, known or not; `false` means it is not one.
    pub(crate) fn resolve(&mut self, message: &Value) -> bool {
        let Some(id) = message.get("id") else {
            return false;
        };
        if message.get("result").is_none() && message.get("error").is_none() {
            return false;
        }

        let Some(waiting) = self.pending.remove(id) else {
            tracing::warn!(id = %id, "Received response for unknown request ID");
            return true;
        };

        let outcome = match message.get("error") {
            Some(error) => Err(error_from_response(error)),
            None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
        };
        let _ = waiting.send(outcome);
        true
    }

    /// Answer every outstanding request with "session closed".
    ///
    /// For a closing connection: no reply can arrive any more, and a handler
    /// awaiting one should learn that now rather than at its timeout.
    pub(crate) fn close(&mut self) {
        if !self.pending.is_empty() {
            tracing::warn!(
                count = self.pending.len(),
                "Abandoning pending server-to-client requests on connection close"
            );
        }
        for (_, waiting) in self.pending.drain() {
            let _ = waiting.send(Err(McpError::internal("Session closed")));
        }
    }
}

/// A client's JSON-RPC error, kept whole — `data` included, since for some
/// errors it is the part the handler needs.
fn error_from_response(error: &Value) -> McpError {
    match serde_json::from_value::<turbomcp_core::jsonrpc::JsonRpcError>(error.clone()) {
        Ok(e) => {
            let mut mapped = McpError::new(ErrorKind::from_i32(e.code), e.message);
            if let Some(data) = e.data {
                mapped = mapped.with_data(data);
            }
            mapped
        }
        Err(_) => McpError::internal("Failed to parse error response"),
    }
}

fn request_frame(id: &Value, method: &str, params: Value) -> Value {
    let mut frame = serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": method });
    if !params.is_null() {
        frame["params"] = params;
    }
    frame
}

/// `params` is omitted when there are none: JSON-RPC requires it to be a
/// structured value when present, and strict peers reject `null`.
fn notification_frame(method: &str, params: Value) -> Value {
    let mut frame = serde_json::json!({ "jsonrpc": "2.0", "method": method });
    if !params.is_null() {
        frame["params"] = params;
    }
    frame
}

/// The `id` of a message that failed validation, if it has a usable one.
///
/// The spec: error responses MUST carry the id of the request they answer,
/// except when it could not be read. An envelope that is wrong in some other
/// way — `"jsonrpc": "1.0"`, a missing `method` — still has a readable id,
/// and answering with `null` leaves the client waiting on it forever.
pub(crate) fn readable_id(message: &Value) -> Option<Value> {
    message
        .get("id")
        .filter(|id| id.is_string() || id.is_i64() || id.is_u64())
        .cloned()
}

/// Cleans up a connection's per-session state however its loop exits.
///
/// Cancels every handler still running — nobody can receive their responses —
/// and clears the log level and rate-limit state kept under the session id.
pub(crate) struct ConnectionCleanup {
    pub(crate) handlers: Arc<DashMap<String, CancellationToken>>,
    pub(crate) session_id: Arc<str>,
}

impl ConnectionCleanup {
    pub(crate) fn new(
        handlers: &Arc<DashMap<String, CancellationToken>>,
        session: &ConnectionSession,
    ) -> Self {
        Self {
            handlers: Arc::clone(handlers),
            session_id: Arc::clone(&session.session_id),
        }
    }
}

impl Drop for ConnectionCleanup {
    fn drop(&mut self) {
        for entry in self.handlers.iter() {
            entry.value().cancel();
        }
        turbomcp_core::context::clear_session_log_state(&self.session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: &str) -> (SessionCommand, oneshot::Receiver<McpResult<Value>>) {
        let (response_tx, response_rx) = oneshot::channel();
        (
            SessionCommand::Request {
                id: Value::String(id.into()),
                method: "sampling/createMessage".into(),
                params: serde_json::json!({}),
                response_tx,
            },
            response_rx,
        )
    }

    /// An abandoned request gives its slot back. Before, 64 unanswered
    /// requests exhausted the connection permanently.
    #[test]
    fn abandoned_requests_do_not_hold_their_slots() {
        let mut outbound = OutboundRequests::default();
        for n in 0..MAX_PENDING_REQUESTS * 2 {
            let (command, response_rx) = request(&format!("s-{n}"));
            assert!(outbound.frame(command).is_some(), "request {n} refused");
            // The handler gave up: timed out, or was cancelled.
            drop(response_rx);
        }
    }

    #[test]
    fn requests_still_awaited_are_capped() {
        let mut outbound = OutboundRequests::default();
        let mut waiting = Vec::new();
        for n in 0..MAX_PENDING_REQUESTS {
            let (command, response_rx) = request(&format!("s-{n}"));
            assert!(outbound.frame(command).is_some());
            waiting.push(response_rx);
        }
        let (command, mut refused) = request("s-over");
        assert!(outbound.frame(command).is_none());
        assert!(refused.try_recv().expect("answered").is_err());
    }

    #[test]
    fn an_error_answer_keeps_its_data() {
        let mut outbound = OutboundRequests::default();
        let (command, mut response_rx) = request("s-1");
        outbound.frame(command);

        assert!(outbound.resolve(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": "s-1",
            "error": { "code": -1, "message": "User rejected", "data": { "why": "no" } }
        })));
        let error = response_rx
            .try_recv()
            .expect("answered")
            .expect_err("error");
        assert_eq!(error.data(), Some(&serde_json::json!({ "why": "no" })));
    }

    #[test]
    fn a_notification_without_params_omits_the_member() {
        let frame = notification_frame("notifications/tools/list_changed", Value::Null);
        assert!(frame.get("params").is_none(), "{frame}");
    }

    /// A handler whose request times out or is dropped tells the client.
    #[tokio::test]
    async fn an_abandoned_call_sends_a_cancellation() {
        let (session, mut commands) = ConnectionSession::new();
        let call = tokio::spawn(async move {
            let _ = session
                .call("elicitation/create", serde_json::json!({}))
                .await;
        });

        let Some(SessionCommand::Request { id, .. }) = commands.recv().await else {
            panic!("expected the request first");
        };
        call.abort();
        let _ = call.await;

        let Some(SessionCommand::Notify { method, params }) = commands.recv().await else {
            panic!("expected a cancellation");
        };
        assert_eq!(method, "notifications/cancelled");
        assert_eq!(params["requestId"], id);
    }
}
