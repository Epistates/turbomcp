//! Resumable response streams on the session wires (`2025-06-18` /
//! `2025-11-25` §Resumability and Redelivery).
//!
//! With an [`EventStore`] configured, a request on a session answers with a
//! primed SSE stream whose events carry ids, `{stream}:{seq}`. The call runs on
//! a task of its own (a disconnect doesn't cancel it on these wires), and that
//! task numbers each message, records it in the store, and hands it to
//! whichever response is attached to the stream right now. A client that lost
//! the connection sends `GET` with `Last-Event-ID`; the endpoint attaches the
//! new response, replays what the store holds after that id, and the stream
//! carries on live until the call's response, which ends it.
//!
//! Attaching and replaying happen under the stream's lock, the same one the
//! task holds while recording and forwarding an event, so a resumed stream
//! neither misses an event nor sees one twice.
//!
//! Without a store nothing here runs, and response streams carry no event
//! ids: an id is a promise of replay, and a promise the endpoint can't keep
//! turns a visible disconnect into a silent gap.

use std::collections::{HashMap, VecDeque};
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use parking_lot::Mutex;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::http::{HeaderName, HeaderValue};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use tokio::sync::mpsc;
use turbomcp_core::JsonRpcMessage;
use turbomcp_core::codec::DefaultCodec;

use super::sse::sse_event;

/// Boxed future returned by [`EventStore`] methods (keeps the trait
/// dyn-compatible, like the other HTTP seams).
pub type EventFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// An [`EventStore`] could not do what was asked.
#[derive(Debug, Clone, thiserror::Error)]
#[error("event store: {0}")]
pub struct EventStoreError(pub String);

/// One recorded event of a stream.
#[derive(Debug, Clone)]
pub struct StoredEvent {
    /// Its position in the stream, from 1.
    pub seq: u64,
    /// What was sent.
    pub message: JsonRpcMessage,
}

/// Where the events of resumable response streams are kept, so a client that
/// lost its connection can be caught up. Configure one with
/// [`HttpConfig::with_event_store`](crate::HttpConfig::with_event_store);
/// [`InMemoryEventStore`] is the bundled one.
///
/// A store outside the process (Redis streams, say) keeps the events across
/// replicas, but a call that is still running lives in one process, so a
/// resume needs the same session-sticky routing the session wires need
/// anyway (see `docs/DEPLOYMENT.md`).
pub trait EventStore: Send + Sync {
    /// Record `message` as event `seq` of `stream` in `session`. Called in
    /// order, once per event.
    fn append<'a>(
        &'a self,
        session: &'a str,
        stream: &'a str,
        seq: u64,
        message: &'a JsonRpcMessage,
    ) -> EventFuture<'a, Result<(), EventStoreError>>;

    /// The events of `stream` with `after < seq < before`, oldest first.
    /// `Ok(None)` when the store can't answer for that range: the stream is
    /// unknown or expired, or events after `after` were dropped. The resume
    /// is then refused rather than served with a gap.
    fn replay<'a>(
        &'a self,
        session: &'a str,
        stream: &'a str,
        after: u64,
        before: u64,
    ) -> EventFuture<'a, Result<Option<Vec<StoredEvent>>, EventStoreError>>;

    /// `stream` got its last event; a store may let it expire.
    fn complete<'a>(
        &'a self,
        session: &'a str,
        stream: &'a str,
    ) -> EventFuture<'a, Result<(), EventStoreError>>;

    /// `session` ended: drop its streams.
    fn forget_session<'a>(
        &'a self,
        session: &'a str,
    ) -> EventFuture<'a, Result<(), EventStoreError>>;
}

struct StreamLog {
    /// Each event with its serialized size.
    events: VecDeque<(StoredEvent, usize)>,
    /// The highest seq dropped to stay within a bound.
    dropped_through: u64,
    completed_at: Option<Instant>,
    /// The serialized size of `events`.
    bytes: usize,
}

impl StreamLog {
    /// Drop the oldest event, returning its size.
    fn drop_oldest(&mut self) -> Option<usize> {
        let (old, size) = self.events.pop_front()?;
        self.dropped_through = old.seq;
        self.bytes -= size;
        Some(size)
    }
}

/// Every stream, and their combined size.
#[derive(Default)]
struct Logs {
    streams: HashMap<(String, String), StreamLog>,
    bytes: usize,
    /// New streams since the last retention sweep.
    since_sweep: usize,
}

/// A bounded in-process [`EventStore`].
///
/// Each stream keeps its last `events_per_stream` events (default 1,024); a
/// resume from before them is refused. A finished stream is kept for
/// `retention` (default 5 minutes), long enough for a client to come back
/// for its response. At most `max_streams` (default 10,000) are kept, and at
/// most `max_bytes` of serialized messages in all (default 64 MiB): past
/// either, finished streams go first, oldest first, then the oldest events
/// of the largest live stream. Worst-case memory is about `max_bytes`, not
/// `events_per_stream × max_streams` messages of any size.
pub struct InMemoryEventStore {
    logs: Mutex<Logs>,
    events_per_stream: usize,
    max_streams: usize,
    max_bytes: usize,
    retention: Duration,
}

impl core::fmt::Debug for InMemoryEventStore {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("InMemoryEventStore")
            .field("events_per_stream", &self.events_per_stream)
            .field("max_streams", &self.max_streams)
            .field("max_bytes", &self.max_bytes)
            .field("retention", &self.retention)
            .finish_non_exhaustive()
    }
}

impl Default for InMemoryEventStore {
    fn default() -> Self {
        Self {
            logs: Mutex::default(),
            events_per_stream: 1024,
            max_streams: 10_000,
            max_bytes: 64 * 1024 * 1024,
            retention: Duration::from_secs(5 * 60),
        }
    }
}

impl InMemoryEventStore {
    /// The defaults: 1,024 events per stream, 10,000 streams, 64 MiB in all,
    /// 5 minutes' retention after a stream finishes.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Keep at most `n` events per stream.
    #[must_use]
    pub fn events_per_stream(mut self, n: usize) -> Self {
        self.events_per_stream = n.max(1);
        self
    }

    /// Keep at most `n` streams.
    #[must_use]
    pub fn max_streams(mut self, n: usize) -> Self {
        self.max_streams = n.max(1);
        self
    }

    /// Keep at most `bytes` of serialized messages across every stream.
    #[must_use]
    pub fn max_bytes(mut self, bytes: usize) -> Self {
        self.max_bytes = bytes;
        self
    }

    /// Keep a finished stream for `retention`.
    #[must_use]
    pub fn retention(mut self, retention: Duration) -> Self {
        self.retention = retention;
        self
    }

    fn lock(&self) -> parking_lot::MutexGuard<'_, Logs> {
        self.logs.lock()
    }

    /// Before a new stream: drop expired finished streams (every 256 new
    /// streams, or when full, not on each one: the sweep is a scan under the
    /// lock), then the oldest finished (or, failing that, any) streams
    /// beyond the bound.
    fn make_room_for_a_stream(&self, logs: &mut Logs) {
        logs.since_sweep += 1;
        if logs.since_sweep >= 256 || logs.streams.len() >= self.max_streams {
            logs.since_sweep = 0;
            let now = Instant::now();
            let mut freed = 0;
            logs.streams.retain(|_, log| {
                let keep = log
                    .completed_at
                    .is_none_or(|at| now.duration_since(at) < self.retention);
                if !keep {
                    freed += log.bytes;
                }
                keep
            });
            logs.bytes -= freed;
        }
        while logs.streams.len() >= self.max_streams {
            let victim = logs
                .streams
                .iter()
                .min_by_key(|(_, log)| (log.completed_at.is_none(), log.completed_at))
                .map(|(key, _)| key.clone());
            match victim.and_then(|key| logs.streams.remove(&key)) {
                Some(log) => logs.bytes -= log.bytes,
                None => break,
            }
        }
    }

    /// Shed events until the store is within `max_bytes`: whole finished
    /// streams, oldest first, then the oldest events of the largest stream.
    fn shed_bytes(&self, logs: &mut Logs) {
        while logs.bytes > self.max_bytes {
            let finished = logs
                .streams
                .iter()
                .filter(|(_, log)| log.completed_at.is_some())
                .min_by_key(|(_, log)| log.completed_at)
                .map(|(key, _)| key.clone());
            if let Some(key) = finished {
                if let Some(log) = logs.streams.remove(&key) {
                    logs.bytes -= log.bytes;
                }
                continue;
            }
            let Some(largest) = logs.streams.values_mut().max_by_key(|log| log.bytes) else {
                break;
            };
            match largest.drop_oldest() {
                Some(size) => logs.bytes -= size,
                None => break,
            }
        }
    }
}

impl EventStore for InMemoryEventStore {
    fn append<'a>(
        &'a self,
        session: &'a str,
        stream: &'a str,
        seq: u64,
        message: &'a JsonRpcMessage,
    ) -> EventFuture<'a, Result<(), EventStoreError>> {
        Box::pin(async move {
            // Sized before the lock: serializing a large result is the costly
            // part, and nothing about it needs the store.
            let size = serde_json::to_vec(message).map_or(0, |bytes| bytes.len());
            let mut logs = self.lock();
            let logs = &mut *logs;
            let key = (session.to_owned(), stream.to_owned());
            if !logs.streams.contains_key(&key) {
                self.make_room_for_a_stream(logs);
            }
            let log = logs.streams.entry(key).or_insert_with(|| StreamLog {
                events: VecDeque::new(),
                dropped_through: 0,
                completed_at: None,
                bytes: 0,
            });
            if size > self.max_bytes {
                // Larger than the whole store: not kept, and a resume from
                // before it is refused rather than silently skipping it.
                log.dropped_through = seq;
                return Ok(());
            }
            log.events.push_back((
                StoredEvent {
                    seq,
                    message: message.clone(),
                },
                size,
            ));
            log.bytes += size;
            logs.bytes += size;
            while log.events.len() > self.events_per_stream {
                if let Some(size) = log.drop_oldest() {
                    logs.bytes -= size;
                }
            }
            self.shed_bytes(logs);
            Ok(())
        })
    }

    fn replay<'a>(
        &'a self,
        session: &'a str,
        stream: &'a str,
        after: u64,
        before: u64,
    ) -> EventFuture<'a, Result<Option<Vec<StoredEvent>>, EventStoreError>> {
        Box::pin(async move {
            let logs = self.lock();
            let Some(log) = logs.streams.get(&(session.to_owned(), stream.to_owned())) else {
                return Ok(None);
            };
            // Expired streams are swept in batches; one past its retention is
            // gone to a resume whether or not the sweep has reached it.
            if log
                .completed_at
                .is_some_and(|at| at.elapsed() >= self.retention)
            {
                return Ok(None);
            }
            if after < log.dropped_through {
                return Ok(None);
            }
            Ok(Some(
                log.events
                    .iter()
                    .map(|(event, _)| event)
                    .filter(|e| e.seq > after && e.seq < before)
                    .cloned()
                    .collect(),
            ))
        })
    }

    fn complete<'a>(
        &'a self,
        session: &'a str,
        stream: &'a str,
    ) -> EventFuture<'a, Result<(), EventStoreError>> {
        Box::pin(async move {
            if let Some(log) = self
                .lock()
                .streams
                .get_mut(&(session.to_owned(), stream.to_owned()))
            {
                log.completed_at = Some(Instant::now());
            }
            Ok(())
        })
    }

    fn forget_session<'a>(
        &'a self,
        session: &'a str,
    ) -> EventFuture<'a, Result<(), EventStoreError>> {
        Box::pin(async move {
            let mut logs = self.lock();
            let logs = &mut *logs;
            let mut freed = 0;
            logs.streams.retain(|(owner, _), log| {
                let keep = owner != session;
                if !keep {
                    freed += log.bytes;
                }
                keep
            });
            logs.bytes -= freed;
            Ok(())
        })
    }
}

/// An event on its way to the response attached to a stream.
pub(super) type Numbered = (u64, JsonRpcMessage);

struct Live {
    next_seq: u64,
    attached: Option<mpsc::Sender<Numbered>>,
    /// Woken when a resume replaces `attached`, so a send stalled on the
    /// replaced response gives up: the new one has the event from the store.
    replaced: Arc<tokio::sync::Notify>,
}

/// One running call's stream, shared by its recorder and any resume.
type LiveStream = Arc<tokio::sync::Mutex<Live>>;

/// What a resume gets: the replayed events and, while the call runs, a
/// receiver for the rest.
type Resumed = (Vec<StoredEvent>, Option<mpsc::Receiver<Numbered>>);

/// The streams whose calls are still running, by `(session, stream)`.
#[derive(Clone, Default)]
pub(super) struct ResumableStreams {
    live: Arc<Mutex<HashMap<(String, String), LiveStream>>>,
}

/// One running call's stream: what its task records and forwards through.
pub(super) struct Recorder {
    streams: ResumableStreams,
    store: Arc<dyn EventStore>,
    session: String,
    stream: String,
    live: LiveStream,
}

impl ResumableStreams {
    /// Open a stream for a call, with `attached` receiving its events.
    pub(super) fn open(
        &self,
        store: Arc<dyn EventStore>,
        session: &str,
        attached: mpsc::Sender<Numbered>,
    ) -> Recorder {
        let stream = uuid::Uuid::new_v4().simple().to_string();
        let live = Arc::new(tokio::sync::Mutex::new(Live {
            next_seq: 1,
            attached: Some(attached),
            replaced: Arc::new(tokio::sync::Notify::new()),
        }));
        self.live
            .lock()
            .insert((session.to_owned(), stream.clone()), Arc::clone(&live));
        Recorder {
            streams: self.clone(),
            store,
            session: session.to_owned(),
            stream,
            live,
        }
    }

    /// Resume `stream` of `session` after event `after`: the events the store
    /// has since then, and (while the call still runs) a receiver for the
    /// rest. `None` when the store can't answer for that range.
    pub(super) async fn resume(
        &self,
        store: &dyn EventStore,
        session: &str,
        stream: &str,
        after: u64,
    ) -> Result<Option<Resumed>, EventStoreError> {
        let live = self
            .live
            .lock()
            .get(&(session.to_owned(), stream.to_owned()))
            .cloned();
        let Some(live) = live else {
            // Finished: the store has all of it.
            return Ok(store
                .replay(session, stream, after, u64::MAX)
                .await?
                .map(|events| (events, None)));
        };
        // Under the stream's lock, which the recorder holds while it records
        // and forwards: events before `next_seq` are in the store, and
        // everything from `next_seq` on comes to the new receiver.
        let mut live = live.lock().await;
        // Caught up already (the client saw the last event, or the stream has
        // none yet): nothing to ask the store, which may hold no record of a
        // stream that has recorded nothing.
        let events = if after.saturating_add(1) >= live.next_seq {
            Vec::new()
        } else {
            let Some(events) = store.replay(session, stream, after, live.next_seq).await? else {
                return Ok(None);
            };
            events
        };
        let (tx, rx) = mpsc::channel(super::sse::SSE_CHANNEL_CAPACITY);
        live.attached = Some(tx);
        live.replaced.notify_waiters();
        Ok(Some((events, Some(rx))))
    }
}

impl Recorder {
    /// The stream's id, as event ids carry it.
    pub(super) fn id(&self) -> &str {
        &self.stream
    }

    /// Whether a response is attached to carry the stream right now.
    pub(super) async fn attached(&self) -> bool {
        self.live
            .lock()
            .await
            .attached
            .as_ref()
            .is_some_and(|tx| !tx.is_closed())
    }

    /// Number `message`, record it, and hand it to the attached response, if
    /// one is still there.
    ///
    /// Numbering and recording happen under the stream's lock, which is what
    /// lets a resume trust that everything before `next_seq` is in the store.
    /// The hand-off doesn't: a response whose reader has stalled used to hold
    /// the lock for as long as it stalled, and so block the very resume that
    /// would replace it.
    pub(super) async fn emit(&self, message: JsonRpcMessage) {
        let (seq, tx, replaced) = {
            let mut live = self.live.lock().await;
            let seq = live.next_seq;
            live.next_seq += 1;
            if let Err(e) = self
                .store
                .append(&self.session, &self.stream, seq, &message)
                .await
            {
                tracing::warn!(error = %e, "could not record an event; it can't be replayed");
            }
            let Some(tx) = live.attached.clone() else {
                return;
            };
            (seq, tx, Arc::clone(&live.replaced))
        };
        // Armed before the lock is released, so a resume that replaces the
        // response between here and the send still wakes it.
        let replaced = replaced.notified();
        tokio::pin!(replaced);
        replaced.as_mut().enable();
        let delivered = tokio::select! {
            sent = tx.send((seq, message)) => sent.is_ok(),
            // Replaced: the resume read this event from the store.
            () = replaced => return,
        };
        if !delivered {
            let mut live = self.live.lock().await;
            // Only if it is still the response that went away: a resume may
            // have attached a new one meanwhile.
            if live
                .attached
                .as_ref()
                .is_some_and(|now| now.same_channel(&tx))
            {
                live.attached = None;
            }
        }
    }

    /// The call is over: mark the stream complete and stop tracking it.
    pub(super) async fn finish(self) {
        if let Err(e) = self.store.complete(&self.session, &self.stream).await {
            tracing::warn!(error = %e, "could not mark a stream complete");
        }
        self.streams
            .live
            .lock()
            .remove(&(self.session.clone(), self.stream.clone()));
    }
}

/// `{stream}:{seq}`.
fn event_id(stream: &str, seq: u64) -> String {
    format!("{stream}:{seq}")
}

/// Split a `Last-Event-ID` back into `(stream, seq)`.
pub(super) fn parse_event_id(id: &str) -> Option<(&str, u64)> {
    let (stream, seq) = id.rsplit_once(':')?;
    Some((stream, seq.parse().ok()?))
}

fn numbered_event(codec: &DefaultCodec, stream: &str, seq: u64, msg: &JsonRpcMessage) -> Event {
    sse_event(codec, msg).id(event_id(stream, seq))
}

/// A resumable response stream: (if `prime`) the priming event, the replayed
/// `events`, then whatever `live` delivers, ending with the call's response.
/// With `polling`, the connection closes after `close_after` with a `retry`
/// field instead, and the client polls the stream back; `hold` (a stream
/// slot, say) lives as long as the connection.
///
/// "The server SHOULD immediately send an SSE event consisting of an event ID
/// and an empty `data` field in order to prime the client to reconnect."
pub(super) fn resumable_stream(
    codec: DefaultCodec,
    stream: String,
    prime: bool,
    events: Vec<Numbered>,
    live: Option<mpsc::Receiver<Numbered>>,
    (keepalive, polling): (Duration, Option<super::SsePolling>),
    hold: Option<Box<dyn Send + Sync>>,
) -> Response {
    let ended = events
        .iter()
        .any(|(_, m)| matches!(m, JsonRpcMessage::Response(_)));
    let mut head: Vec<Result<Event, Infallible>> = Vec::new();
    head.extend(
        events
            .iter()
            .map(|(seq, msg)| Ok(numbered_event(&codec, &stream, *seq, msg))),
    );
    let tail_stream = stream.clone();
    // "If the server does close the connection prior to terminating the SSE
    // stream, it SHOULD send an SSE event with a standard `retry` field
    // before closing the connection."
    let deadline = polling.map(|p| (tokio::time::Instant::now() + p.close_after, p.retry));
    let tail = futures::stream::unfold(live.filter(|_| !ended).map(|rx| (rx, hold)), move |live| {
        let stream = tail_stream.clone();
        async move {
            let (mut rx, hold) = live?;
            let next = match deadline {
                Some((at, retry)) => tokio::select! {
                    next = rx.recv() => next,
                    () = tokio::time::sleep_until(at) => {
                        return Some((Ok(Event::default().retry(retry).comment("poll")), None));
                    }
                },
                None => rx.recv().await,
            };
            let (seq, msg) = next?;
            let event = numbered_event(&codec, &stream, seq, &msg);
            let next = (!matches!(msg, JsonRpcMessage::Response(_))).then_some((rx, hold));
            Some((Ok::<_, Infallible>(event), next))
        }
    });
    let body = futures::StreamExt::chain(futures::stream::iter(head), tail);
    let sse = Sse::new(body).keep_alive(KeepAlive::new().interval(keepalive).text("keep-alive"));
    let response = (
        [(
            HeaderName::from_static("x-accel-buffering"),
            HeaderValue::from_static("no"),
        )],
        sse,
    )
        .into_response();
    if !prime {
        return response;
    }
    // The priming event is "an event ID and an empty data field", written
    // out by hand: axum drops an empty `data`, and an event with no data
    // field at all is one most SSE parsers never surface, id included, so
    // the client would have nothing to resume from.
    let (parts, body) = response.into_parts();
    let primer = Bytes::from(format!("id: {}\ndata: \n\n", event_id(&stream, 0)));
    let body = futures::StreamExt::chain(
        futures::stream::once(async move { Ok::<_, axum::Error>(primer) }),
        body.into_data_stream(),
    );
    Response::from_parts(parts, axum::body::Body::from_stream(body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp_core::JsonRpcNotification;

    fn note(n: u64) -> JsonRpcMessage {
        JsonRpcNotification::new(format!("n{n}"), None).into()
    }

    /// A notification whose serialized form is about `bytes` long.
    fn sized(bytes: usize) -> JsonRpcMessage {
        JsonRpcNotification::new("n", Some(serde_json::json!({ "pad": "x".repeat(bytes) }))).into()
    }

    /// The store stays within its byte budget: finished streams go first,
    /// then the oldest events of the largest live one.
    #[tokio::test]
    async fn the_store_keeps_to_its_byte_budget() {
        let store = InMemoryEventStore::new().max_bytes(10_000);
        for seq in 1..=4 {
            store.append("s", "done", seq, &sized(2_000)).await.unwrap();
        }
        store.complete("s", "done").await.unwrap();
        for seq in 1..=4 {
            store.append("s", "live", seq, &sized(2_000)).await.unwrap();
        }
        assert!(store.lock().bytes <= 10_000);
        assert!(
            store
                .replay("s", "done", 0, u64::MAX)
                .await
                .unwrap()
                .is_none(),
            "the finished stream went first"
        );
        assert_eq!(
            store
                .replay("s", "live", 0, u64::MAX)
                .await
                .unwrap()
                .unwrap()
                .len(),
            4
        );
        // Now only the live stream can give way, oldest event first.
        for seq in 5..=6 {
            store.append("s", "live", seq, &sized(2_000)).await.unwrap();
        }
        assert!(store.lock().bytes <= 10_000);
        assert!(
            store
                .replay("s", "live", 0, u64::MAX)
                .await
                .unwrap()
                .is_none(),
            "a resume from before the shed events is refused"
        );
        let kept = store
            .replay("s", "live", 2, u64::MAX)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(kept.last().unwrap().seq, 6);

        // One message larger than the whole budget isn't kept at all.
        store.append("s", "huge", 1, &sized(20_000)).await.unwrap();
        assert!(
            store
                .replay("s", "huge", 0, u64::MAX)
                .await
                .unwrap()
                .is_none()
        );
        store.forget_session("s").await.unwrap();
        assert_eq!(store.lock().bytes, 0, "the byte count follows removals");
    }

    #[tokio::test]
    async fn the_store_replays_a_range_and_refuses_a_gap() {
        let store = InMemoryEventStore::new().events_per_stream(3);
        for seq in 1..=5 {
            store.append("s", "a", seq, &note(seq)).await.unwrap();
        }
        let seqs = |events: Vec<StoredEvent>| events.iter().map(|e| e.seq).collect::<Vec<_>>();
        assert_eq!(
            seqs(store.replay("s", "a", 3, 5).await.unwrap().unwrap()),
            [4]
        );
        assert_eq!(
            seqs(store.replay("s", "a", 2, u64::MAX).await.unwrap().unwrap()),
            [3, 4, 5]
        );
        assert!(
            store.replay("s", "a", 1, u64::MAX).await.unwrap().is_none(),
            "event 2 was dropped, so a resume from 1 would have a gap"
        );
        assert!(store.replay("other", "a", 0, 9).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_finished_stream_expires_and_sessions_are_forgotten() {
        let store = InMemoryEventStore::new()
            .retention(Duration::ZERO)
            .max_streams(10);
        store.append("s", "a", 1, &note(1)).await.unwrap();
        store.complete("s", "a").await.unwrap();
        store.append("s", "b", 1, &note(1)).await.unwrap();
        assert!(
            store.replay("s", "a", 0, 9).await.unwrap().is_none(),
            "retention passed"
        );
        store.forget_session("s").await.unwrap();
        assert!(store.replay("s", "b", 0, 9).await.unwrap().is_none());
    }

    #[test]
    fn event_ids_round_trip() {
        assert_eq!(parse_event_id(&event_id("abc", 7)), Some(("abc", 7)));
        assert_eq!(parse_event_id("garbage"), None);
        assert_eq!(parse_event_id("abc:x"), None);
    }
}
