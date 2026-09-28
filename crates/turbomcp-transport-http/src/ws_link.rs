//! JSON-RPC over a WebSocket: one frame per message, in both directions.
//! Shared by the server route (axum's socket) and the client transport
//! (tungstenite's).
//!
//! Writes run on a task of their own behind a bounded queue, so a frame the
//! peer is slow to take never stops this side reading, which is how a stdio
//! pipe deadlocked before it got the same treatment. The keepalive's state
//! lives in the link, not in the `recv` future: both drivers drop and recreate
//! that future every time another branch wins, and a count kept in it reset
//! on every outbound write, so a connection with steady outbound traffic was
//! never probed at all.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use futures::stream::SplitStream;
use futures::{Sink, SinkExt, Stream, StreamExt};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use turbomcp_core::codec::{Codec, CodecError, DefaultCodec, decode_message};
use turbomcp_core::{InvalidFrame, JsonRpcMessage};
use turbomcp_service::{CancellationToken, Transport};

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Failures on a WebSocket link.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WsError {
    /// The socket failed: I/O, the WebSocket protocol, or the handshake.
    #[error("websocket error: {0}")]
    Socket(#[source] BoxError),
    /// A frame could not be encoded.
    #[error("codec error: {0}")]
    Codec(#[from] CodecError),
    /// An outbound frame was not valid UTF-8 (a text frame must be).
    #[error("outbound frame was not valid UTF-8")]
    Utf8,
    /// One inbound message wasn't a valid JSON-RPC message. Recoverable:
    /// WebSocket framing delivers the next one intact.
    #[error("invalid frame: {0}")]
    InvalidFrame(InvalidFrame),
    /// The writer stopped (the socket closed under it).
    #[error("the websocket writer has stopped")]
    Closed,
}

impl From<WsError> for turbomcp_service::ProtocolError {
    fn from(err: WsError) -> Self {
        match err {
            WsError::Codec(e) => Self::Parse(e.to_string()),
            WsError::InvalidFrame(frame) => Self::Parse(frame.to_string()),
            other => Self::Transport(format!("websocket: {other}")),
        }
    }
}

/// What an inbound message carries.
pub(crate) enum Read<'a> {
    /// A text or binary frame: one JSON-RPC message.
    Data(&'a [u8]),
    /// The peer closed.
    Close,
    /// Ping, pong or a raw frame: the library answers pings itself.
    Skip,
}

/// The one thing that differs between axum's and tungstenite's sockets.
pub(crate) trait Frame: Send + Sized + 'static {
    fn text(frame: Bytes) -> Result<Self, WsError>;
    fn ping() -> Self;
    fn close(code: u16, reason: &'static str) -> Self;
    fn read(&self) -> Read<'_>;
}

/// Close codes (RFC 6455 §7.4.1).
pub(crate) mod close {
    pub(crate) const NORMAL: u16 = 1000;
    pub(crate) const GOING_AWAY: u16 = 1001;
    #[cfg(feature = "server")]
    pub(crate) const POLICY: u16 = 1008;
}

/// Probe a connection that has sent nothing for `interval`, and give up on
/// it after `max_idle_pings` unanswered probes (`None` pings forever).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Keepalive {
    pub(crate) interval: Duration,
    pub(crate) max_idle_pings: Option<u32>,
}

/// The code a link closes with, settable after the link has been handed to
/// a driver (a credential expiring mid-connection).
#[derive(Clone, Debug)]
pub(crate) struct CloseWith(Arc<Mutex<Option<(u16, &'static str)>>>);

impl CloseWith {
    #[cfg(feature = "server")]
    pub(crate) fn set(&self, code: u16, reason: &'static str) {
        *self.0.lock().expect("close code lock") = Some((code, reason));
    }

    fn get(&self) -> Option<(u16, &'static str)> {
        *self.0.lock().expect("close code lock")
    }
}

/// Frames waiting for the writer before `send` waits too.
const WRITE_QUEUE: usize = 64;

pub(crate) struct Link<M, R> {
    frames: R,
    out: mpsc::Sender<M>,
    writer: JoinHandle<Result<(), BoxError>>,
    codec: DefaultCodec,
    keepalive: Option<Keepalive>,
    last_inbound: Instant,
    idle_pings: u32,
    close_with: CloseWith,
    going_away: Option<CancellationToken>,
}

impl<M, S, E> Link<M, SplitStream<S>>
where
    M: Frame,
    S: Stream<Item = Result<M, E>> + Sink<M, Error = E> + Send + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    /// Split `socket` and start its writer.
    pub(crate) fn new(socket: S, keepalive: Option<Keepalive>) -> Self {
        let (sink, frames) = socket.split();
        let (out, queue) = mpsc::channel(WRITE_QUEUE);
        Self {
            frames,
            out,
            writer: tokio::spawn(write(sink, queue)),
            codec: DefaultCodec::default(),
            keepalive,
            last_inbound: Instant::now(),
            idle_pings: 0,
            close_with: CloseWith(Arc::default()),
            going_away: None,
        }
    }
}

impl<M, R> Link<M, R> {
    #[cfg(feature = "client")]
    pub(crate) fn set_keepalive(&mut self, keepalive: Option<Keepalive>) {
        self.keepalive = keepalive;
    }

    /// Where to set the code this link closes with.
    #[cfg(feature = "server")]
    pub(crate) fn close_with(&self) -> CloseWith {
        self.close_with.clone()
    }

    /// Close with `1001 Going Away` rather than `1000` once `token` fires:
    /// the peer should know the server left, not that the exchange finished.
    #[cfg(feature = "server")]
    pub(crate) fn going_away_on(mut self, token: CancellationToken) -> Self {
        self.going_away = Some(token);
        self
    }
}

async fn write<M, K>(mut sink: K, mut queue: mpsc::Receiver<M>) -> Result<(), BoxError>
where
    K: Sink<M> + Unpin,
    K::Error: std::error::Error + Send + Sync + 'static,
{
    while let Some(frame) = queue.recv().await {
        sink.feed(frame).await?;
        if queue.is_empty() {
            sink.flush().await?;
        }
    }
    sink.close().await?;
    Ok(())
}

impl<M, R, E> Link<M, R>
where
    M: Frame,
    R: Stream<Item = Result<M, E>> + Unpin + Send + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    async fn next_frame(&mut self) -> Result<Option<M>, WsError> {
        loop {
            let next = match self.keepalive {
                None => self.frames.next().await,
                Some(keepalive) => {
                    let deadline = self.last_inbound + keepalive.interval * (self.idle_pings + 1);
                    match tokio::time::timeout_at(deadline, self.frames.next()).await {
                        Ok(next) => next,
                        Err(_idle) => {
                            if keepalive
                                .max_idle_pings
                                .is_some_and(|max| self.idle_pings >= max)
                            {
                                tracing::debug!(
                                    idle_pings = self.idle_pings,
                                    "websocket peer answered no pings; closing"
                                );
                                return Ok(None);
                            }
                            self.idle_pings += 1;
                            // A full queue means the writer is busy; the next
                            // interval probes again.
                            let _ = self.out.try_send(M::ping());
                            continue;
                        }
                    }
                }
            };
            // Anything inbound, a pong included, proves the peer is there.
            self.last_inbound = Instant::now();
            self.idle_pings = 0;
            return match next {
                None => Ok(None),
                Some(frame) => frame.map(Some).map_err(|e| WsError::Socket(Box::new(e))),
            };
        }
    }
}

impl<M, R, E> Transport for Link<M, R>
where
    M: Frame,
    R: Stream<Item = Result<M, E>> + Unpin + Send + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    type Error = WsError;

    fn invalid_frame(error: WsError) -> Result<InvalidFrame, WsError> {
        match error {
            WsError::InvalidFrame(frame) => Ok(frame),
            other => Err(other),
        }
    }

    async fn send(&mut self, msg: JsonRpcMessage) -> Result<(), WsError> {
        let frame = M::text(self.codec.encode(&msg)?)?;
        if self.out.send(frame).await.is_err() {
            return Err(WsError::Closed);
        }
        Ok(())
    }

    /// # Cancel safety
    /// Safe to drop: a frame is either taken whole or left in the stream, and
    /// the keepalive's deadline and count live in the link.
    async fn recv(&mut self) -> Result<Option<JsonRpcMessage>, WsError> {
        loop {
            let Some(frame) = self.next_frame().await? else {
                return Ok(None);
            };
            match frame.read() {
                Read::Data(bytes) => {
                    return decode_message(&self.codec, bytes)
                        .map(Some)
                        .map_err(WsError::InvalidFrame);
                }
                Read::Close => return Ok(None),
                Read::Skip => {}
            }
        }
    }

    /// Best-effort: a peer that already closed gets nothing more, and that is
    /// not a failure of this side.
    async fn close(self) -> Result<(), WsError> {
        let (code, reason) = self.close_with.get().unwrap_or_else(|| {
            if self
                .going_away
                .as_ref()
                .is_some_and(CancellationToken::is_cancelled)
            {
                (close::GOING_AWAY, "server shutting down")
            } else {
                (close::NORMAL, "")
            }
        });
        let _ = self.out.send(M::close(code, reason)).await;
        drop(self.out);
        if let Ok(Err(e)) = self.writer.await {
            tracing::debug!(error = %e, "websocket close did not complete cleanly");
        }
        Ok(())
    }
}
