//! The STDIO transport: newline-delimited JSON frames over a process's stdin
//! (inbound) and stdout (outbound) — the transport Claude Desktop and most local
//! MCP launchers speak. Framing (split on `\n`) is this module's job; turning a
//! frame's bytes into a value is the [`Codec`]'s.
//!
//! The framing lives in [`LineTransport`], generic over any async byte streams,
//! so it is unit-testable over an in-memory pipe. [`StdioTransport`]/[`stdio`]
//! specialize it to stdin/stdout; [`serve_stdio`] pairs it with a service (the
//! dispatcher).

use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader, Stdin, Stdout,
};
use turbomcp_core::codec::{Codec, CodecError, DefaultCodec, decode_message};
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

/// Newline-delimited JSON-RPC over any async reader/writer pair.
///
/// Each inbound line is one complete frame (blank lines are skipped); each
/// outbound frame is written followed by `\n` and flushed. Stdio is the
/// `R = BufReader<Stdin>`, `W = Stdout` specialization ([`StdioTransport`]).
///
/// Inbound lines are bounded by [`DEFAULT_MAX_LINE_BYTES`] (override with
/// [`with_max_line_bytes`](Self::with_max_line_bytes)) so a peer cannot exhaust
/// memory with an endless unterminated line.
pub struct LineTransport<R, W, C = DefaultCodec> {
    reader: R,
    writer: W,
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

impl<R, W, C: Codec> LineTransport<R, W, C> {
    /// Build a transport over `reader`/`writer` with the given codec and the
    /// default per-line cap ([`DEFAULT_MAX_LINE_BYTES`]).
    pub fn new(reader: R, writer: W, codec: C) -> Self {
        Self {
            reader,
            writer,
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

    async fn send(&mut self, msg: JsonRpcMessage) -> Result<(), Self::Error> {
        let bytes = self.codec.encode(&msg)?;
        self.writer.write_all(bytes.as_ref()).await?;
        self.writer.write_all(b"\n").await?;
        self.writer.flush().await?;
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

    async fn close(mut self) -> Result<(), Self::Error> {
        self.writer.flush().await?;
        Ok(())
    }
}

/// Newline-delimited JSON over the process's stdin/stdout.
pub type StdioTransport<C = DefaultCodec> = LineTransport<BufReader<Stdin>, Stdout, C>;

/// A [`StdioTransport`] over the process's stdin/stdout with the [`DefaultCodec`].
#[must_use]
pub fn stdio() -> StdioTransport {
    LineTransport::new(
        BufReader::new(tokio::io::stdin()),
        tokio::io::stdout(),
        DefaultCodec::default(),
    )
}

/// Serve `service` over stdin/stdout until the peer closes stdin, exactly as
/// given. For a stateful (`initialize`-handshake) client to work, `service`
/// has to be wrapped in the server's `LegacySessionAdapter`, which stamps the
/// connection's session onto every later request; the facade's
/// `ServeStdio::run_stdio` does that for you.
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
