//! The SSE response plumbing: per-request streams, listen and `GET` streams.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use axum::http::{HeaderName, HeaderValue};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use turbomcp_core::codec::{Codec, DefaultCodec};
use turbomcp_core::{ConnectionId, JsonRpcMessage, McpRequest, RequestId};
use turbomcp_service::{CancellationToken, Peer, ProtocolError, StreamGuard};

use super::streams::StreamSlot;

/// Buffered events per SSE stream; a consumer this far behind backpressures
/// publishers (the registry awaits `send`).
pub(super) const SSE_CHANNEL_CAPACITY: usize = 256;

/// What keeps one response stream's queue open: the only strong sender (every
/// [`Peer`] holds it weakly), for a session `GET` stream its registry entry,
/// and for a long-lived stream its [`StreamSlot`]. It travels inside the
/// stream state, so a client disconnect (axum drops the body) closes the
/// queue and returns the slot, and anything still holding the stream's `Peer`
/// sees it closed.
pub(super) struct Outlet {
    pub(super) _tx: tokio::sync::mpsc::Sender<JsonRpcMessage>,
    pub(super) _guard: Option<StreamGuard>,
    pub(super) _slot: Option<StreamSlot>,
}

impl Outlet {
    /// Open a stream for `request`: a fresh queue under a minted connection id
    /// (`prefix-uuid`), attached to the request as its connection and `Peer`.
    pub(super) fn open(
        prefix: &str,
        request: &mut McpRequest,
    ) -> (Self, tokio::sync::mpsc::Receiver<JsonRpcMessage>) {
        let (tx, rx) = tokio::sync::mpsc::channel::<JsonRpcMessage>(SSE_CHANNEL_CAPACITY);
        let id = ConnectionId::new(format!("{prefix}-{}", uuid::Uuid::new_v4()));
        request.extensions.insert(id.clone());
        request.extensions.insert(Peer::new(id, &tx));
        (
            Self {
                _tx: tx,
                _guard: None,
                _slot: None,
            },
            rx,
        )
    }
}

/// State for an upgraded per-request SSE response: keep streaming channel
/// messages while driving the in-flight call; when the call completes, append
/// its final response and end the stream.
pub(super) enum PostStream<F> {
    /// The request is still in flight.
    Run {
        rx: tokio::sync::mpsc::Receiver<JsonRpcMessage>,
        call: Pin<Box<F>>,
        id: RequestId,
        registration: Outlet,
    },
    /// The call finished; flush the remaining events and close.
    Tail(VecDeque<JsonRpcMessage>),
}

/// The upgraded per-request SSE response (see [`request_post`]). The final
/// response (or a JSON-RPC error built from a [`ProtocolError`]) is the last
/// event; dropping the response body drops the call future.
pub(super) fn streaming_post_sse<F>(
    codec: DefaultCodec,
    first: Option<JsonRpcMessage>,
    run: PostStream<F>,
    keepalive: Duration,
) -> Response
where
    F: Future<Output = Result<Option<JsonRpcMessage>, ProtocolError>> + Send + 'static,
{
    let head = futures::stream::iter(first.map(|m| Ok::<_, Infallible>(sse_event(&codec, &m))));
    let tail = futures::stream::unfold(run, move |state| async move {
        match state {
            PostStream::Run {
                mut rx,
                mut call,
                id,
                registration,
            } => {
                tokio::select! {
                    result = call.as_mut() => {
                        let mut events = drain(&mut rx);
                        drop(registration);
                        match result {
                            Ok(Some(reply)) => events.push_back(reply),
                            Ok(None) => {}
                            Err(e) => events.push_back(e.into_response(id).into()),
                        }
                        let msg = events.pop_front()?;
                        Some((
                            Ok::<_, Infallible>(sse_event(&codec, &msg)),
                            PostStream::Tail(events),
                        ))
                    }
                    msg = rx.recv() => {
                        let msg = msg.expect("sender held by registration");
                        Some((
                            Ok::<_, Infallible>(sse_event(&codec, &msg)),
                            PostStream::Run { rx, call, id, registration },
                        ))
                    }
                }
            }
            PostStream::Tail(mut events) => {
                let msg = events.pop_front()?;
                Some((
                    Ok::<_, Infallible>(sse_event(&codec, &msg)),
                    PostStream::Tail(events),
                ))
            }
        }
    });
    let stream = futures::StreamExt::chain(head, tail);
    let sse = Sse::new(stream).keep_alive(KeepAlive::new().interval(keepalive).text("keep-alive"));
    (
        [(
            HeaderName::from_static("x-accel-buffering"),
            HeaderValue::from_static("no"),
        )],
        sse,
    )
        .into_response()
}

/// A short, complete SSE response for a request that finished before the
/// upgrade decision but raced messages into its channel: the messages, the
/// final response, end of stream.
pub(super) fn finished_sse(codec: DefaultCodec, events: VecDeque<JsonRpcMessage>) -> Response {
    let stream = futures::stream::iter(
        events
            .into_iter()
            .map(move |msg| sse_event(&codec, &msg))
            .map(Ok::<_, Infallible>),
    );
    (
        [(
            HeaderName::from_static("x-accel-buffering"),
            HeaderValue::from_static("no"),
        )],
        Sse::new(stream),
    )
        .into_response()
}

/// Empty the channel without awaiting (post-completion stragglers).
pub(super) fn drain(
    rx: &mut tokio::sync::mpsc::Receiver<JsonRpcMessage>,
) -> VecDeque<JsonRpcMessage> {
    let mut events = VecDeque::new();
    while let Ok(msg) = rx.try_recv() {
        events.push_back(msg);
    }
    events
}

/// Encode one message as one `data:` event; an encode failure becomes a
/// comment so the stream survives.
pub(super) fn sse_event(codec: &DefaultCodec, msg: &JsonRpcMessage) -> Event {
    match codec.encode(msg) {
        Ok(bytes) => Event::default().data(String::from_utf8_lossy(&bytes)),
        Err(e) => {
            tracing::warn!(error = %e, "failed to encode SSE event; skipped");
            Event::default().comment("event encoding failed; skipped")
        }
    }
}

/// The common SSE response shape for the two long-lived stream kinds (modern
/// listen, legacy GET): every channel message becomes one `data:` event;
/// keep-alive comments flow in between; the writer registration travels inside
/// the stream state so dropping the response body unregisters it.
///
/// The stream ends on a JSON-RPC response (a listen's graceful close) or when
/// `close` fires: at shutdown for both kinds, and for a `GET` stream also when
/// a newer one replaces it or its session ends. The subscriptions spec ends a
/// subscription by closing its stream, and "the server MAY close the SSE
/// stream at any time" on the legacy wire; a `GET` stream left open at
/// shutdown held the drain for its full timeout, since only the client could
/// end it.
pub(super) fn sse_response(
    codec: DefaultCodec,
    rx: tokio::sync::mpsc::Receiver<JsonRpcMessage>,
    registration: Outlet,
    keepalive: Duration,
    close: CancellationToken,
) -> Response {
    let stream = futures::stream::unfold(
        (Some((rx, registration)), codec, close),
        |(live, codec, close)| async move {
            let (mut rx, registration) = live?;
            let msg = tokio::select! {
                () = close.cancelled() => None,
                m = rx.recv() => m,
            }?;
            let event = sse_event(&codec, &msg);
            // A JSON-RPC *response* ends the stream — the per-POST stream
            // contract ("the final response ends the stream").
            let next = if matches!(msg, JsonRpcMessage::Response(_)) {
                None
            } else {
                Some((rx, registration))
            };
            Some((Ok::<_, Infallible>(event), (next, codec, close)))
        },
    );

    let sse = Sse::new(stream).keep_alive(KeepAlive::new().interval(keepalive).text("keep-alive"));
    // X-Accel-Buffering tells reverse proxies (nginx) not to buffer the
    // stream (transports spec: SHOULD include it on SSE responses).
    (
        [(
            HeaderName::from_static("x-accel-buffering"),
            HeaderValue::from_static("no"),
        )],
        sse,
    )
        .into_response()
}
