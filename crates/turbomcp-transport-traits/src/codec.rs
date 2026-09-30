//! Newline-delimited framing with a size limit that skips, rather than ends
//! on, an oversized line.
//!
//! Shared by the stdio, TCP and Unix socket transports.

use bytes::BytesMut;
use tokio_util::codec::{Decoder, Encoder, LinesCodec, LinesCodecError};

/// One newline-delimited frame from the peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// A complete line within the size limit.
    Message(String),
    /// A line past the size limit, already discarded up to its newline.
    Oversized,
}

/// `LinesCodec` that reports an oversized line as a frame, not an error.
///
/// `LinesCodec::new_with_max_length` already discards a line past the limit
/// and resynchronises at the next newline, but it says so with an error, and
/// `FramedRead` treats every decoder error as the end of the stream. Surfacing
/// it as [`Line::Oversized`] lets one oversized message be skipped instead of
/// ending the reader and every request still in flight.
#[derive(Debug)]
pub struct BoundedLines(LinesCodec);

impl BoundedLines {
    /// A codec that holds at most `max_length` bytes of a line.
    #[must_use]
    pub fn new(max_length: usize) -> Self {
        Self(LinesCodec::new_with_max_length(max_length))
    }

    fn lift(
        decoded: Result<Option<String>, LinesCodecError>,
    ) -> Result<Option<Line>, LinesCodecError> {
        match decoded {
            Err(LinesCodecError::MaxLineLengthExceeded) => Ok(Some(Line::Oversized)),
            other => other.map(|line| line.map(Line::Message)),
        }
    }
}

impl Decoder for BoundedLines {
    type Item = Line;
    type Error = LinesCodecError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Line>, LinesCodecError> {
        Self::lift(self.0.decode(src))
    }

    fn decode_eof(&mut self, src: &mut BytesMut) -> Result<Option<Line>, LinesCodecError> {
        Self::lift(self.0.decode_eof(src))
    }
}

/// Writes a line and its newline, so one codec can drive a `Framed` stream.
impl Encoder<String> for BoundedLines {
    type Error = LinesCodecError;

    fn encode(&mut self, line: String, dst: &mut BytesMut) -> Result<(), LinesCodecError> {
        self.0.encode(line, dst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_oversized_line_is_a_frame_and_the_next_one_still_decodes() {
        let mut codec = BoundedLines::new(8);
        let mut buffer = BytesMut::from("far too long for eight\nok\n");

        assert_eq!(codec.decode(&mut buffer).unwrap(), Some(Line::Oversized));
        assert_eq!(
            codec.decode(&mut buffer).unwrap(),
            Some(Line::Message("ok".into()))
        );
    }
}
