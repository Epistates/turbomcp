//! WebSocket transport implementation

use async_trait::async_trait;
use bytes::Bytes;
use futures::{SinkExt as _, StreamExt as _};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use turbomcp_core::MessageId;

use crate::core::{
    Transport, TransportCapabilities, TransportError, TransportMessage, TransportMetrics,
    TransportResult, TransportState, TransportType,
};

/// WebSocket transport implementation
#[derive(Debug)]
pub struct WebSocketTransport {
    stream: Option<WebSocketStream<MaybeTlsStream<TcpStream>>>,
}

impl WebSocketTransport {
    /// Create a new WebSocket transport
    pub async fn new(url: &str) -> TransportResult<Self> {
        let (stream, _) = connect_async(url)
            .await
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;

        Ok(Self {
            stream: Some(stream),
        })
    }

    /// Create a new WebSocket transport without connection (for testing)
    #[doc(hidden)]
    #[must_use]
    pub const fn new_disconnected() -> Self {
        Self { stream: None }
    }
}

#[async_trait]
impl Transport for WebSocketTransport {
    fn transport_type(&self) -> TransportType {
        TransportType::WebSocket
    }

    fn capabilities(&self) -> &TransportCapabilities {
        use std::sync::LazyLock;
        static CAPABILITIES: LazyLock<TransportCapabilities> =
            LazyLock::new(|| TransportCapabilities {
                max_message_size: Some(16 * 1024 * 1024), // 16MB
                supports_compression: true,
                supports_streaming: true,
                supports_bidirectional: true,
                supports_multiplexing: false,
                compression_algorithms: vec![],
                custom: std::collections::HashMap::new(),
            });
        &CAPABILITIES
    }

    async fn state(&self) -> TransportState {
        if self.stream.is_some() {
            TransportState::Connected
        } else {
            TransportState::Disconnected
        }
    }

    async fn connect(&mut self) -> TransportResult<()> {
        // WebSocket connection is established in new()
        Ok(())
    }

    async fn disconnect(&mut self) -> TransportResult<()> {
        if let Some(mut stream) = self.stream.take() {
            stream
                .close(None)
                .await
                .map_err(|e| TransportError::ConnectionLost(e.to_string()))?;
        }
        Ok(())
    }

    async fn send(&mut self, message: TransportMessage) -> TransportResult<()> {
        if let Some(ref mut stream) = self.stream {
            let text = String::from_utf8(message.payload.to_vec())
                .map_err(|e| TransportError::SendFailed(e.to_string()))?;

            stream
                .send(Message::Text(text.into()))
                .await
                .map_err(|e| TransportError::SendFailed(e.to_string()))?;

            Ok(())
        } else {
            Err(TransportError::SendFailed(
                "WebSocket not connected".to_string(),
            ))
        }
    }

    async fn receive(&mut self) -> TransportResult<Option<TransportMessage>> {
        if let Some(ref mut stream) = self.stream {
            match stream.next().await {
                Some(Ok(Message::Text(text))) => {
                    let id = MessageId::from(uuid::Uuid::new_v4()); // Generate a new message ID
                    let payload = Bytes::from(text);
                    Ok(Some(TransportMessage::new(id, payload)))
                }
                Some(Ok(Message::Close(_))) => Err(TransportError::ReceiveFailed(
                    "WebSocket closed".to_string(),
                )),
                Some(Err(e)) => Err(TransportError::ReceiveFailed(e.to_string())),
                None => Err(TransportError::ReceiveFailed(
                    "WebSocket stream ended".to_string(),
                )),
                _ => {
                    Ok(None) // Ignore other message types
                }
            }
        } else {
            Err(TransportError::ReceiveFailed(
                "WebSocket not connected".to_string(),
            ))
        }
    }

    async fn metrics(&self) -> TransportMetrics {
        TransportMetrics::default()
    }
}
