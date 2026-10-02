//! The STDIO transport: newline-delimited JSON frames over a process's stdin
//! (inbound) and stdout (outbound) — the transport Claude Desktop and most local
//! MCP launchers speak. Framing (split on `\n`) is this module's job; turning a
//! frame's bytes into a value is the [`Codec`]'s.
//!
//! The framing lives in [`LineTransport`], generic over any async byte streams,
//! so it is unit-testable over an in-memory pipe. [`StdioTransport`]/[`stdio`]
//! specialize it to stdin/stdout; [`serve_stdio`] pairs it with a service (the
//! dispatcher).

use futures::stream::BoxStream;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader, Stdout};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::io::StreamReader;
use turbomcp_core::codec::{Bytes, Codec, CodecError, DefaultCodec, decode_message};
use turbomcp_core::{InvalidFrame, JsonRpcMessage};

use crate::{McpService, ProtocolError, ServeConfig, Transport};

/// Failures from the line transport.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StdioError {
    /// An I/O error on the underlying stream.
    #[error("stdio i/o error: {0}")]
    Io(#[from] std::io::Error),
    /// A frame could not be encoded.
    #[error("codec error: {0}")]
    Codec(#[from] CodecError),
    /// One inbound line wasn't a valid message. Recoverable: the next line is
    /// the next frame.
    #[error("invalid frame: {0}")]
    InvalidFrame(InvalidFrame),
    /// A single inbound line exceeded [`LineTransport`]'s configured maximum.
    /// Recoverable: the rest of the line is discarded unread, never buffered,
    /// and reading resumes after its newline.
    #[error("inbound line exceeded the {max}-byte maximum")]
    LineTooLong {
        /// The configured per-line cap, in bytes.
        max: usize,
    },
}

/// Default cap on one inbound line (a single JSON-RPC frame), in bytes.
///
/// A line longer than this is refused with [`StdioError::LineTooLong`] and
/// skipped rather than growing the read buffer without bound, so a peer that
/// streams bytes and never sends `\n` can't force an unbounded allocation. 64 MiB clears any realistic MCP frame (including base64
/// image/audio payloads) while bounding the worst case; tune with
/// [`LineTransport::with_max_line_bytes`].
pub const DEFAULT_MAX_LINE_BYTES: usize = 64 * 1024 * 1024;

/// How many encoded frames may wait for the writer before
/// [`send`](Transport::send) waits too.
const WRITE_QUEUE_FRAMES: usize = 64;

/// Newline-delimited JSON-RPC over any async reader/writer pair.
///
/// Each inbound line is one complete frame (blank lines are skipped); each
/// outbound frame is written followed by `\n`. Stdio is the
/// `R = BufReader<Stdin>`, `W = Stdout` specialization ([`StdioTransport`]).
///
/// Writes happen on a task of their own, started by the first `send`, so a
/// frame bigger than the pipe buffer never stops this side reading. When both
/// peers wrote inline, each could block writing a large frame to a peer that
/// was itself blocked writing, and neither drained its input. `send` queues up
/// to a bounded number of frames and then waits, which is the backpressure;
/// [`close`](Transport::close) writes out whatever is queued.
///
/// Inbound lines are bounded by [`DEFAULT_MAX_LINE_BYTES`] (override with
/// [`with_max_line_bytes`](Self::with_max_line_bytes)) so a peer cannot exhaust
/// memory with an endless unterminated line.
pub struct LineTransport<R, W, C = DefaultCodec> {
    reader: R,
    writer: Writer<W>,
    codec: C,
    buf: Vec<u8>,
    max_line_bytes: usize,
    /// Set after an overlong line: skip to its newline before reading on. A
    /// field, not a local, so a `recv` dropped mid-skip resumes it.
    discarding: bool,
}

impl<R, W, C> core::fmt::Debug for LineTransport<R, W, C> {
    /// Deliberately unbounded in `R`/`W`/`C`: a derive would demand `Debug` of
    /// whichever byte streams the user plugged in, so a struct holding a
    /// transport could not derive its own.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LineTransport")
            .field("max_line_bytes", &self.max_line_bytes)
            .field("buffered", &self.buf.len())
            .finish_non_exhaustive()
    }
}

/// The outbound half: the byte stream until the first `send`, then the queue
/// in front of the task that owns it.
enum Writer<W> {
    Idle(W),
    Running {
        queue: mpsc::Sender<Bytes>,
        task: JoinHandle<std::io::Result<()>>,
    },
    /// The task has ended and its error was reported.
    Stopped,
}

impl<W: AsyncWrite + Unpin + Send + 'static> Writer<W> {
    /// The queue to the writer task, starting the task on first use. Started
    /// lazily so a transport can be built before a runtime exists.
    fn queue(&mut self) -> Result<&mpsc::Sender<Bytes>, StdioError> {
        if matches!(self, Self::Idle(_)) {
            let Self::Idle(writer) = std::mem::replace(self, Self::Stopped) else {
                unreachable!()
            };
            let (queue, frames) = mpsc::channel(WRITE_QUEUE_FRAMES);
            let task = tokio::spawn(write_frames(writer, frames));
            *self = Self::Running { queue, task };
        }
        match self {
            Self::Running { queue, .. } => Ok(queue),
            _ => Err(writer_stopped()),
        }
    }

    /// Why the writer task ended, once it has.
    async fn failure(&mut self) -> StdioError {
        match std::mem::replace(self, Self::Stopped) {
            Self::Running { queue, task } => {
                drop(queue);
                match task.await {
                    Ok(Err(e)) => StdioError::Io(e),
                    Ok(Ok(())) => writer_stopped(),
                    Err(join) => StdioError::Io(std::io::Error::other(join)),
                }
            }
            _ => writer_stopped(),
        }
    }
}

fn writer_stopped() -> StdioError {
    StdioError::Io(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "the line writer has stopped",
    ))
}

/// Write queued frames until the queue closes. Flushes whenever the queue runs
/// dry, so a burst goes out in one flush and a lone frame isn't held back.
async fn write_frames<W: AsyncWrite + Unpin>(
    mut writer: W,
    mut frames: mpsc::Receiver<Bytes>,
) -> std::io::Result<()> {
    while let Some(frame) = frames.recv().await {
        writer.write_all(&frame).await?;
        writer.write_all(b"\n").await?;
        if frames.is_empty() {
            writer.flush().await?;
        }
    }
    writer.flush().await
}

impl<R, W, C: Codec> LineTransport<R, W, C> {
    /// Build a transport over `reader`/`writer` with the given codec and the
    /// default per-line cap ([`DEFAULT_MAX_LINE_BYTES`]).
    pub fn new(reader: R, writer: W, codec: C) -> Self {
        Self {
            reader,
            writer: Writer::Idle(writer),
            codec,
            buf: Vec::new(),
            max_line_bytes: DEFAULT_MAX_LINE_BYTES,
            discarding: false,
        }
    }

    /// Cap a single inbound line at `max` bytes; a longer line is refused with
    /// [`StdioError::LineTooLong`] and skipped. Lower this when serving
    /// untrusted peers with small expected frames; raise it for large trusted
    /// payloads. `0` is treated as `1` (a cap of at least one byte).
    #[must_use]
    pub fn with_max_line_bytes(mut self, max: usize) -> Self {
        self.max_line_bytes = max.max(1);
        self
    }
}

/// Outcome of one bounded line read.
enum LineRead {
    /// A line was read into the buffer (its trailing `\n`, if any, included).
    Line,
    /// Clean end-of-stream with nothing buffered.
    Eof,
    /// The line would exceed the cap; reading stopped.
    TooLong,
}

/// Consume bytes up to and including the next `\n` without keeping them.
/// `Ok(true)` once the newline is found, `Ok(false)` at end of stream.
async fn skip_line<R>(reader: &mut R) -> Result<bool, std::io::Error>
where
    R: AsyncBufRead + Unpin,
{
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(false);
        }
        match available.iter().position(|&b| b == b'\n') {
            Some(i) => {
                reader.consume(i + 1);
                return Ok(true);
            }
            None => {
                let n = available.len();
                reader.consume(n);
            }
        }
    }
}

/// Read one `\n`-terminated line into `buf`, never letting `buf` grow past
/// `max` bytes. Unlike [`AsyncBufReadExt::read_line`], an unterminated flood is
/// bounded: once the accumulated bytes would exceed `max` we stop and report
/// [`LineRead::TooLong`] instead of allocating without limit.
async fn read_line_capped<R>(
    reader: &mut R,
    buf: &mut Vec<u8>,
    max: usize,
) -> Result<LineRead, std::io::Error>
where
    R: AsyncBufRead + Unpin,
{
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            // EOF: a final line without a trailing newline still counts.
            return Ok(if buf.is_empty() {
                LineRead::Eof
            } else {
                LineRead::Line
            });
        }
        let (take, done) = match available.iter().position(|&b| b == b'\n') {
            Some(i) => (i + 1, true), // include the newline
            None => (available.len(), false),
        };
        if buf.len() + take > max {
            return Ok(LineRead::TooLong); // don't consume; the caller aborts
        }
        buf.extend_from_slice(&available[..take]);
        reader.consume(take);
        if done {
            return Ok(LineRead::Line);
        }
    }
}

impl<R, W, C> Transport for LineTransport<R, W, C>
where
    R: AsyncBufRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
    C: Codec,
{
    type Error = StdioError;

    fn invalid_frame(error: Self::Error) -> Result<InvalidFrame, Self::Error> {
        match error {
            StdioError::InvalidFrame(frame) => Ok(frame),
            StdioError::LineTooLong { max } => Ok(InvalidFrame::too_large(max)),
            other => Err(other),
        }
    }

    /// Queues the frame for the writer task; waits only while the queue is
    /// full. A write error surfaces on the next `send` (or `close`).
    async fn send(&mut self, msg: JsonRpcMessage) -> Result<(), Self::Error> {
        let frame = self.codec.encode(&msg)?;
        if self.writer.queue()?.send(frame).await.is_err() {
            return Err(self.writer.failure().await);
        }
        Ok(())
    }

    /// # Cancel safety
    /// Safe to drop part-way through a line. `self.buf` is the *resumable*
    /// accumulator rather than per-call scratch, which is what makes that true:
    /// both drivers poll this as one branch of a `select!` in a loop, so the
    /// future is dropped every time another branch wins, and `read_line_capped`
    /// has already consumed those bytes from the reader. Clearing on entry
    /// would discard them and hand the next call a truncated line, which
    /// decodes as garbage. The same goes for skipping an overlong line:
    /// `discarding` is a field so a dropped skip picks up where it left off.
    async fn recv(&mut self) -> Result<Option<JsonRpcMessage>, Self::Error> {
        loop {
            if self.discarding {
                if !skip_line(&mut self.reader).await? {
                    return Ok(None);
                }
                self.discarding = false;
            }
            match read_line_capped(&mut self.reader, &mut self.buf, self.max_line_bytes).await? {
                LineRead::Eof => return Ok(None),
                LineRead::TooLong => {
                    self.buf.clear();
                    self.discarding = true;
                    return Err(StdioError::LineTooLong {
                        max: self.max_line_bytes,
                    });
                }
                LineRead::Line => {
                    // Decode before clearing, so the line leaves the buffer on
                    // every path out of here and the accumulator keeps its
                    // allocation for the next one.
                    let decoded = match self.buf.trim_ascii() {
                        [] => None, // tolerate blank keep-alive lines
                        trimmed => Some(decode_message(&self.codec, trimmed)),
                    };
                    self.buf.clear();
                    match decoded {
                        None => continue,
                        Some(Ok(msg)) => return Ok(Some(msg)),
                        Some(Err(frame)) => return Err(StdioError::InvalidFrame(frame)),
                    }
                }
            }
        }
    }

    /// Writes out everything queued, then flushes.
    async fn close(self) -> Result<(), Self::Error> {
        match self.writer {
            Writer::Idle(mut writer) => writer.flush().await?,
            Writer::Running { queue, task } => {
                drop(queue);
                task.await.map_err(std::io::Error::other)??;
            }
            Writer::Stopped => {}
        }
        Ok(())
    }
}

/// Newline-delimited JSON over the process's stdin/stdout.
pub type StdioTransport<C = DefaultCodec> = LineTransport<BufReader<StdinReader>, Stdout, C>;

/// A [`StdioTransport`] over the process's stdin/stdout with the [`DefaultCodec`].
#[must_use]
pub fn stdio() -> StdioTransport {
    LineTransport::new(
        BufReader::new(StdinReader::new()),
        tokio::io::stdout(),
        DefaultCodec::default(),
    )
}

/// The process's stdin, read on a thread of its own.
///
/// `tokio::io::stdin()` reads on the runtime's blocking pool with an ordinary
/// read that can't be cancelled, and dropping a runtime "will block
/// indefinitely for spawned blocking tasks". A server whose shutdown token
/// fired returned from `serve` and then hung at the end of `main` until the
/// client wrote another line or closed the pipe, so the client had to kill
/// it. A plain thread holds nothing the runtime waits for: it stays blocked in
/// its read and ends with the process.
pub struct StdinReader(StreamReader<BoxStream<'static, std::io::Result<Bytes>>, Bytes>);

impl core::fmt::Debug for StdinReader {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("StdinReader").finish_non_exhaustive()
    }
}

impl StdinReader {
    /// Start reading the process's stdin.
    ///
    /// # Panics
    /// If the OS refuses to start a thread.
    #[must_use]
    pub fn new() -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel::<std::io::Result<Bytes>>(4);
        std::thread::Builder::new()
            .name("turbomcp-stdin".into())
            .spawn(move || {
                use std::io::Read as _;
                let mut stdin = std::io::stdin().lock();
                let mut buf = vec![0u8; 8192];
                loop {
                    let chunk = match stdin.read(&mut buf) {
                        Ok(0) => break, // EOF: dropping `tx` ends the stream
                        Ok(n) => Ok(Bytes::copy_from_slice(&buf[..n])),
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(e) => Err(e),
                    };
                    let failed = chunk.is_err();
                    if tx.blocking_send(chunk).is_err() || failed {
                        break;
                    }
                }
            })
            .expect("start the stdin reader thread");
        let chunks = futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|chunk| (chunk, rx))
        });
        Self(StreamReader::new(Box::pin(chunks)))
    }
}

impl Default for StdinReader {
    fn default() -> Self {
        Self::new()
    }
}

impl tokio::io::AsyncRead for StdinReader {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().0).poll_read(cx, buf)
    }
}

/// Serve `service` over stdin/stdout until the peer closes stdin, exactly as
/// given. For a stateful (`initialize`-handshake) client to work, `service`
/// has to be wrapped in the server's `LegacySessionAdapter`, which stamps the
/// connection's session onto every later request; the facade's
/// `ServerBuilder::serve(stdio())` does that for you.
///
/// # Errors
/// Propagates transport and service errors from the driver loop.
pub async fn serve_stdio<S>(service: S) -> Result<(), ProtocolError>
where
    S: McpService + Clone,
    S::Future: Send + 'static,
{
    crate::serve(stdio(), service).await
}

/// Serve `service` over stdin/stdout with explicit [`ServeConfig`] — the entry
/// point when you need a shutdown token, drain timeout, or concurrency bound.
///
/// # Errors
/// Propagates transport and service errors from the driver loop.
pub async fn serve_stdio_with<S>(service: S, config: ServeConfig) -> Result<(), ProtocolError>
where
    S: McpService + Clone,
    S::Future: Send + 'static,
{
    crate::serve_with(stdio(), service, config).await
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn transport(
        input: &'static [u8],
    ) -> LineTransport<BufReader<&'static [u8]>, Vec<u8>, DefaultCodec> {
        LineTransport::new(BufReader::new(input), Vec::new(), DefaultCodec::default())
    }

    const PING: &[u8] = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n";

    #[tokio::test]
    async fn reads_a_framed_line_then_clean_eof() {
        let mut t = transport(PING);
        assert!(matches!(
            t.recv().await.unwrap(),
            Some(JsonRpcMessage::Request(_))
        ));
        assert!(
            t.recv().await.unwrap().is_none(),
            "clean EOF after the frame"
        );
    }

    #[tokio::test]
    async fn final_line_without_newline_still_parses() {
        // No trailing `\n`: the EOF path must still yield the buffered frame.
        let mut t = transport(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}");
        assert!(matches!(
            t.recv().await.unwrap(),
            Some(JsonRpcMessage::Request(_))
        ));
    }

    #[tokio::test]
    async fn blank_keepalive_lines_are_skipped() {
        let mut t = transport(b"\n  \n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n");
        assert!(matches!(
            t.recv().await.unwrap(),
            Some(JsonRpcMessage::Request(_))
        ));
    }

    /// One bad line costs one frame: the error is recoverable and the next
    /// line reads normally.
    #[tokio::test]
    async fn a_bad_line_is_recoverable_and_the_next_one_reads() {
        let mut t = transport(
            b"Server starting...\n{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"x\",\"params\":\"s\"}\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n",
        );
        let banner =
            LineTransport::<BufReader<&[u8]>, Vec<u8>>::invalid_frame(t.recv().await.unwrap_err())
                .expect("recoverable");
        assert_eq!(banner.code, turbomcp_core::codes::PARSE_ERROR);
        let params =
            LineTransport::<BufReader<&[u8]>, Vec<u8>>::invalid_frame(t.recv().await.unwrap_err())
                .expect("recoverable");
        assert_eq!(params.id, Some(turbomcp_core::RequestId::Number(7)));
        assert!(matches!(
            t.recv().await.unwrap(),
            Some(JsonRpcMessage::Request(_))
        ));
    }

    /// An overlong line is refused without being buffered, and the frame
    /// after it still arrives.
    #[tokio::test]
    async fn an_overlong_line_is_skipped_and_reading_resumes() {
        let mut input = vec![b'a'; 4096];
        input.push(b'\n');
        input.extend_from_slice(PING);
        let input: &'static [u8] = input.leak();
        let mut t = LineTransport::new(BufReader::new(input), Vec::new(), DefaultCodec::default())
            .with_max_line_bytes(64);
        assert!(matches!(
            t.recv().await.unwrap_err(),
            StdioError::LineTooLong { max: 64 }
        ));
        assert!(t.buf.capacity() <= 64, "the flood was never buffered");
        assert!(matches!(
            t.recv().await.unwrap(),
            Some(JsonRpcMessage::Request(_))
        ));
    }

    #[tokio::test]
    async fn an_unterminated_flood_is_rejected_not_buffered() {
        // A single 4 KiB line with no `\n`, against an 8-byte cap: the read must
        // stop and error rather than growing the buffer to hold it all (the
        // memory-DoS guard for untrusted socket peers).
        let flood: &'static [u8] = vec![b'a'; 4096].leak();
        let mut t = LineTransport::new(BufReader::new(flood), Vec::new(), DefaultCodec::default())
            .with_max_line_bytes(8);
        assert!(matches!(
            t.recv().await.unwrap_err(),
            StdioError::LineTooLong { max: 8 }
        ));
    }

    #[tokio::test]
    async fn a_frame_at_the_cap_still_parses() {
        // The cap bounds the flood but must not reject a legitimate frame that
        // fits: PING is well under 4 KiB.
        let mut t = transport(PING).with_max_line_bytes(4096);
        assert!(matches!(
            t.recv().await.unwrap(),
            Some(JsonRpcMessage::Request(_))
        ));
    }

    /// A frame far bigger than the pipe doesn't stop this side reading: the
    /// peer here writes before it reads, as a sequential server loop does,
    /// and when `send` wrote inline both sides blocked on a full pipe.
    #[tokio::test]
    async fn a_large_write_does_not_block_reading() {
        let (ours, theirs) = tokio::io::duplex(8 * 1024);
        let (our_read, our_write) = tokio::io::split(ours);
        let (their_read, mut their_write) = tokio::io::split(theirs);
        let mut t =
            LineTransport::new(BufReader::new(our_read), our_write, DefaultCodec::default());

        let big = "x".repeat(1024 * 1024);
        let peer = tokio::spawn(async move {
            let reply =
                format!("{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{\"blob\":\"{big}\"}}}}\n");
            their_write.write_all(reply.as_bytes()).await.unwrap();
            let mut line = String::new();
            BufReader::new(their_read)
                .read_line(&mut line)
                .await
                .unwrap();
            line.len()
        });

        let request = JsonRpcMessage::Request(turbomcp_core::JsonRpcRequest::new(
            2,
            "tools/call",
            Some(serde_json::json!({ "blob": "y".repeat(1024 * 1024) })),
        ));
        tokio::time::timeout(Duration::from_secs(5), async {
            t.send(request).await.expect("queued");
            let reply = t.recv().await.expect("read").expect("a frame");
            assert!(matches!(reply, JsonRpcMessage::Response(_)));
        })
        .await
        .expect("both directions progressed");
        assert!(
            peer.await.unwrap() > 1024 * 1024,
            "the peer got the whole frame"
        );
        t.close().await.expect("closed");
    }

    /// `close` writes out what is still queued.
    #[tokio::test]
    async fn close_flushes_the_queue() {
        let (ours, mut theirs) = tokio::io::duplex(64 * 1024);
        let mut t = LineTransport::new(
            BufReader::new(tokio::io::empty()),
            ours,
            DefaultCodec::default(),
        );
        for id in 0..10 {
            t.send(turbomcp_core::JsonRpcRequest::new(id, "ping", None).into())
                .await
                .unwrap();
        }
        t.close().await.unwrap();
        let mut out = String::new();
        tokio::io::AsyncReadExt::read_to_string(&mut theirs, &mut out)
            .await
            .unwrap();
        assert_eq!(out.lines().count(), 10);
    }

    /// `recv` must be cancel safe, because both drivers poll it as one branch
    /// of a `select!` in a loop: every time another branch wins, this future is
    /// dropped part-way through a line. The bytes it already took are gone from
    /// the reader, so they have to survive in the transport and the next call
    /// has to resume on top of them.
    #[tokio::test]
    async fn a_partly_read_line_survives_the_future_being_dropped() {
        let (mut peer, io) = tokio::io::duplex(64);
        let mut t = LineTransport::new(BufReader::new(io), Vec::new(), DefaultCodec::default());

        // Half a frame, no newline: `recv` consumes it and then waits.
        peer.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"me")
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), t.recv())
                .await
                .is_err(),
            "the frame is incomplete, so recv should still be waiting"
        );

        // The rest arrives after the dropped future would have discarded it.
        peer.write_all(b"thod\":\"ping\"}\n").await.unwrap();
        let msg = t
            .recv()
            .await
            .expect("the resumed read must not see a truncated line")
            .expect("a frame");
        let JsonRpcMessage::Request(req) = msg else {
            panic!("expected a request");
        };
        assert_eq!(req.method, "ping");
    }
}
