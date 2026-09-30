//! [`Peer`] — where server-initiated messages to one connection go.
//!
//! A transport's writer is the only thing allowed to write its wire, so
//! everything else that needs to reach the client (subscription notifications,
//! progress, logs, inline elicitation) goes through a handle to that writer's
//! queue. The transport attaches a [`Peer`] to every request it hands the
//! service, and whatever outlives the request (a subscription, a task) keeps
//! the handle.
//!
//! A `Peer` holds its queue *weakly*: the transport owns the strong sender for
//! as long as the connection is open, and a `Peer` whose connection has gone
//! simply reports [`Delivery::Closed`]. So a subscription left behind by a
//! departed client can neither keep its connection alive nor write to a
//! stranger, and the transport's shutdown drain isn't held open by one.
//!
//! This replaces a process-global table keyed by connection-id strings, which
//! every connection of every server in the process shared: any code holding an
//! id could write to that connection, and tests collided on hard-coded ids.
//!
//! [`SessionStreams`] is the one piece of shared lookup left: a stateful
//! Streamable HTTP session's standalone `GET` stream can be opened (and
//! reopened) after the request that subscribed, so a publish has to find the
//! session's *current* stream. The HTTP transport owns one registry per
//! endpoint and attaches it to that endpoint's requests.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;
use turbomcp_core::{CancellationToken, ConnectionId, JsonRpcMessage};

/// A handle to one connection's (or one response stream's) ordered outbound
/// queue. Cheap to clone; never keeps the connection open.
#[derive(Clone)]
pub struct Peer {
    id: ConnectionId,
    tx: mpsc::WeakSender<JsonRpcMessage>,
}

impl core::fmt::Debug for Peer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Peer")
            .field("id", &self.id)
            .field("open", &self.is_open())
            .finish()
    }
}

/// The connection behind a [`Peer`] is gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the connection is closed")]
pub struct PeerClosed;

/// How handing a broadcast message to one connection went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// Queued on the connection's writer.
    Queued,
    /// The connection's queue is full: it has stopped reading. The message
    /// was dropped for this connection only.
    Dropped,
    /// The connection is gone.
    Closed,
}

impl Peer {
    /// A handle to `tx`'s queue, for the connection `id`. The caller keeps
    /// `tx`: the queue stays open for exactly as long as it does.
    #[must_use]
    pub fn new(id: impl Into<ConnectionId>, tx: &mpsc::Sender<JsonRpcMessage>) -> Self {
        Self {
            id: id.into(),
            tx: tx.downgrade(),
        }
    }

    /// The connection this handle writes to.
    #[must_use]
    pub fn id(&self) -> &ConnectionId {
        &self.id
    }

    /// Whether the connection is still open.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.tx.upgrade().is_some_and(|tx| !tx.is_closed())
    }

    /// Send a message that belongs to one of this connection's requests (a
    /// reply, progress, a log line, an inline request), waiting for queue
    /// space: for a request's own connection, waiting is the backpressure.
    ///
    /// # Errors
    /// [`PeerClosed`] when the connection has gone.
    pub async fn send(&self, msg: JsonRpcMessage) -> Result<(), PeerClosed> {
        let tx = self.tx.upgrade().ok_or(PeerClosed)?;
        tx.send(msg).await.map_err(|_| PeerClosed)
    }

    /// Reserve room for one message, waiting for it as [`send`](Self::send)
    /// does, so it can be queued later without waiting: under a lock, where
    /// it has to land in the same step as something else.
    ///
    /// # Errors
    /// [`PeerClosed`] when the connection has gone.
    pub async fn reserve(&self) -> Result<Reserved, PeerClosed> {
        let tx = self.tx.upgrade().ok_or(PeerClosed)?;
        tx.reserve_owned()
            .await
            .map(Reserved)
            .map_err(|_| PeerClosed)
    }

    /// Hand a broadcast notification over without waiting.
    ///
    /// A fan-out that awaited each writer in turn let one connection that
    /// stopped reading stall delivery to every other one, and park the task
    /// that published. Broadcast notifications (`*/list_changed`,
    /// `resources/updated`, task status) are "something changed, look again"
    /// hints, and a reader that far behind already has one queued, so a full
    /// queue drops the message for this connection instead.
    pub fn offer(&self, msg: JsonRpcMessage) -> Delivery {
        let Some(tx) = self.tx.upgrade() else {
            return Delivery::Closed;
        };
        match tx.try_send(msg) {
            Ok(()) => Delivery::Queued,
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::warn!(
                    connection = %self.id,
                    "connection is not reading its stream; dropped a broadcast notification for it"
                );
                Delivery::Dropped
            }
            Err(mpsc::error::TrySendError::Closed(_)) => Delivery::Closed,
        }
    }
}

/// Room for one message on a [`Peer`]'s queue, from [`Peer::reserve`].
/// Holding it keeps the connection's queue open, so use it promptly.
#[derive(Debug)]
pub struct Reserved(mpsc::OwnedPermit<JsonRpcMessage>);

impl Reserved {
    /// Queue `msg` in the reserved slot. Never waits.
    pub fn send(self, msg: JsonRpcMessage) {
        self.0.send(msg);
    }
}

/// Each stateful HTTP session's current standalone `GET` stream.
///
/// One stream per session also enforces the spec's "MUST NOT broadcast the
/// same message across multiple streams": a newer `GET` replaces the older
/// registration, and the older stream is told to end. Clones share the
/// registry.
#[derive(Clone, Default)]
pub struct SessionStreams {
    streams: Arc<Mutex<HashMap<String, Registered>>>,
}

struct Registered {
    peer: Peer,
    close: CancellationToken,
}

impl core::fmt::Debug for SessionStreams {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SessionStreams")
            .field("sessions", &self.lock().len())
            .finish()
    }
}

impl SessionStreams {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Make `peer` the stream for `session` until the guard drops. `close` is
    /// the stream's own end signal: it fires when a newer `GET` replaces this
    /// one or the session is [closed](Self::close), and the transport ends
    /// the stream on it. Any earlier stream's `close` fires now.
    ///
    /// Ending the replaced stream matters because the registry is not what
    /// keeps a stream open: its response body is. A superseded stream the
    /// registry forgot kept its connection (and its keep-alives) for as long
    /// as the client held it.
    #[must_use = "dropping the guard immediately unregisters the stream"]
    pub fn register(
        &self,
        session: impl Into<String>,
        peer: Peer,
        close: CancellationToken,
    ) -> StreamGuard {
        let session = session.into();
        let id = peer.id().clone();
        if let Some(old) = self
            .lock()
            .insert(session.clone(), Registered { peer, close })
        {
            old.close.cancel();
        }
        StreamGuard {
            streams: self.clone(),
            session,
            id,
        }
    }

    /// The session's current stream, if one is open.
    #[must_use]
    pub fn get(&self, session: &str) -> Option<Peer> {
        self.lock()
            .get(session)
            .map(|r| &r.peer)
            .filter(|p| p.is_open())
            .cloned()
    }

    /// End `session`'s stream, if it has one: the session is over (the client
    /// deleted it, or it expired), and a stream left open would hold its
    /// connection with nothing ever to deliver.
    pub fn close(&self, session: &str) {
        if let Some(old) = self.lock().remove(session) {
            old.close.cancel();
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Registered>> {
        self.streams
            .lock()
            .expect("session stream registry poisoned")
    }
}

/// Unregisters its stream when dropped, and only its own: a reconnecting
/// `GET` stream replaces the previous one, and the old guard dropping a moment
/// later must not remove the new one (it used to, leaving a live stream the
/// server could no longer write to).
#[derive(Debug)]
pub struct StreamGuard {
    streams: SessionStreams,
    session: String,
    id: ConnectionId,
}

impl Drop for StreamGuard {
    fn drop(&mut self) {
        let mut streams = self.streams.lock();
        if streams
            .get(&self.session)
            .is_some_and(|r| r.peer.id() == &self.id)
        {
            streams.remove(&self.session);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp_core::JsonRpcNotification;

    fn note() -> JsonRpcMessage {
        JsonRpcNotification::new("ping", None).into()
    }

    /// A peer never keeps its connection's queue open: once the owner drops
    /// the sender, the peer reports closed, and the receiver sees the end.
    #[tokio::test]
    async fn a_peer_does_not_outlive_its_connection() {
        let (tx, mut rx) = mpsc::channel(4);
        let peer = Peer::new("conn-1", &tx);
        assert!(peer.is_open());
        peer.send(note()).await.expect("open");
        assert!(rx.recv().await.is_some());

        drop(tx);
        assert!(!peer.is_open());
        assert_eq!(peer.offer(note()), Delivery::Closed);
        assert_eq!(peer.send(note()).await, Err(PeerClosed));
        assert!(rx.recv().await.is_none(), "the queue closed with its owner");
    }

    #[test]
    fn a_full_queue_drops_a_broadcast_for_that_connection_only() {
        let (tx, _rx) = mpsc::channel(1);
        let peer = Peer::new("slow", &tx);
        assert_eq!(peer.offer(note()), Delivery::Queued);
        assert_eq!(peer.offer(note()), Delivery::Dropped);
    }

    /// A reconnecting stream replaces the old one, and the old guard dropping
    /// afterwards leaves the new one registered.
    #[test]
    fn a_replaced_stream_survives_the_old_guards_drop() {
        let streams = SessionStreams::new();
        let (first_tx, _first_rx) = mpsc::channel(1);
        let (second_tx, mut second_rx) = mpsc::channel(1);
        let first_close = CancellationToken::new();
        let first = streams.register("s-1", Peer::new("get-1", &first_tx), first_close.clone());
        let _second = streams.register(
            "s-1",
            Peer::new("get-2", &second_tx),
            CancellationToken::new(),
        );
        assert!(
            first_close.is_cancelled(),
            "the replaced stream is told to end"
        );
        drop(first);

        let current = streams.get("s-1").expect("the reconnected stream");
        assert_eq!(current.id().as_str(), "get-2");
        assert_eq!(current.offer(note()), Delivery::Queued);
        assert!(second_rx.try_recv().is_ok());
    }

    #[test]
    fn a_closed_stream_is_not_returned() {
        let streams = SessionStreams::new();
        let (tx, _rx) = mpsc::channel(1);
        let _guard = streams.register("s-2", Peer::new("get", &tx), CancellationToken::new());
        drop(tx);
        assert!(streams.get("s-2").is_none());
    }

    /// A session that ends takes its stream with it.
    #[test]
    fn closing_a_session_ends_its_stream() {
        let streams = SessionStreams::new();
        let (tx, _rx) = mpsc::channel(1);
        let close = CancellationToken::new();
        let _guard = streams.register("s-3", Peer::new("get", &tx), close.clone());
        streams.close("s-3");
        assert!(close.is_cancelled());
        assert!(streams.get("s-3").is_none());
        streams.close("s-3");
    }
}
