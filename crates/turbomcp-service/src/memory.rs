//! An in-memory transport pair: the two ends of one connection, in one
//! process. For tests, and for embedding a server in the program that uses
//! it.
//!
//! Every frame is encoded on one end and decoded on the other, as on a real
//! wire, so a value that wouldn't survive serialization fails here too rather
//! than first in production. A frame that doesn't decode is answered like a
//! bad stdio line: the connection carries on.
//!
//! ```ignore
//! let (server_end, client_end) = turbomcp_service::memory::pair();
//! tokio::spawn(MyServer.into_server().serve(server_end));
//! let client = ClientBuilder::new("test", "1.0.0").connect(client_end).await?;
//! ```

use tokio::sync::mpsc;
use turbomcp_core::codec::{Bytes, Codec, CodecError, DefaultCodec, decode_message};
use turbomcp_core::{InvalidFrame, JsonRpcMessage};

use crate::{NetworkFacts, Transport};

/// Frames one end may have in flight before [`send`](Transport::send) waits
/// for the other to read.
const CAPACITY: usize = 256;

/// One end of an in-memory connection; see [`pair`].
#[derive(Debug)]
pub struct MemoryTransport {
    tx: mpsc::Sender<Bytes>,
    rx: mpsc::Receiver<Bytes>,
    codec: DefaultCodec,
}

/// What can go wrong on an in-memory connection.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MemoryError {
    /// The other end is gone.
    #[error("the other end of the in-memory connection closed")]
    Closed,
    /// A frame could not be encoded.
    #[error("codec error: {0}")]
    Codec(#[from] CodecError),
    /// One inbound frame wasn't a valid message. Recoverable.
    #[error("invalid frame: {0}")]
    InvalidFrame(InvalidFrame),
}

/// The two ends of a new in-memory connection: hand one to a server, the
/// other to a client.
#[must_use]
pub fn pair() -> (MemoryTransport, MemoryTransport) {
    let (a_tx, b_rx) = mpsc::channel(CAPACITY);
    let (b_tx, a_rx) = mpsc::channel(CAPACITY);
    (
        MemoryTransport {
            tx: a_tx,
            rx: a_rx,
            codec: DefaultCodec::default(),
        },
        MemoryTransport {
            tx: b_tx,
            rx: b_rx,
            codec: DefaultCodec::default(),
        },
    )
}

impl Transport for MemoryTransport {
    type Error = MemoryError;

    fn network(&self) -> Option<NetworkFacts> {
        Some(NetworkFacts::pipe())
    }

    fn invalid_frame(error: MemoryError) -> Result<InvalidFrame, MemoryError> {
        match error {
            MemoryError::InvalidFrame(frame) => Ok(frame),
            other => Err(other),
        }
    }

    async fn send(&mut self, msg: JsonRpcMessage) -> Result<(), MemoryError> {
        let frame = self.codec.encode(&msg)?;
        self.tx.send(frame).await.map_err(|_| MemoryError::Closed)
    }

    /// # Cancel safety
    /// Safe: a frame stays in the channel until `recv` returns it.
    async fn recv(&mut self) -> Result<Option<JsonRpcMessage>, MemoryError> {
        match self.rx.recv().await {
            None => Ok(None),
            Some(frame) => decode_message(&self.codec, &frame)
                .map(Some)
                .map_err(MemoryError::InvalidFrame),
        }
    }

    async fn close(self) -> Result<(), MemoryError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp_core::{JsonRpcNotification, JsonRpcRequest};

    #[tokio::test]
    async fn frames_cross_and_close_ends_the_other_side() {
        let (mut a, mut b) = pair();
        a.send(JsonRpcRequest::new(1, "ping", None).into())
            .await
            .unwrap();
        let Some(JsonRpcMessage::Request(r)) = b.recv().await.unwrap() else {
            panic!("expected the request");
        };
        assert_eq!(r.method, "ping");
        b.send(JsonRpcNotification::new("n", None).into())
            .await
            .unwrap();
        assert!(matches!(
            a.recv().await.unwrap(),
            Some(JsonRpcMessage::Notification(_))
        ));
        a.close().await.unwrap();
        assert!(b.recv().await.unwrap().is_none(), "end of stream");
        assert!(matches!(
            b.send(JsonRpcNotification::new("n", None).into()).await,
            Err(MemoryError::Closed)
        ));
    }

    #[tokio::test]
    async fn a_bad_frame_is_recoverable() {
        let (a, mut b) = pair();
        a.tx.send(Bytes::from_static(b"not json")).await.unwrap();
        let err = b.recv().await.unwrap_err();
        assert!(MemoryTransport::invalid_frame(err).is_ok());
    }
}
