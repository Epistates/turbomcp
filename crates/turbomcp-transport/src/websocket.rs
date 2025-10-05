//! WebSocket transport implementation

use async_trait::async_trait;
use bytes::Bytes;
use futures::{SinkExt as _, StreamExt as _};
use std::sync::Arc;
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
    stream: Arc<tokio::sync::Mutex<Option<WebSocketStream<MaybeTlsStream<TcpStream>>>>>,
}

impl WebSocketTransport {
    /// Create a new WebSocket transport
    pub async fn new(url: &str) -> TransportResult<Self> {
        let (stream, _) = connect_async(url)
            .await
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;

        Ok(Self {
            stream: Arc::new(tokio::sync::Mutex::new(Some(stream))),
        })
    }

    /// Create a new WebSocket transport without connection (for testing)
    #[doc(hidden)]
    #[must_use]
    pub fn new_disconnected() -> Self {
        Self {
            stream: Arc::new(tokio::sync::Mutex::new(None)),
        }
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
        if self.stream.lock().await.is_some() {
            TransportState::Connected
        } else {
            TransportState::Disconnected
        }
    }

    async fn connect(&self) -> TransportResult<()> {
        // WebSocket connection is established in new()
        Ok(())
    }

    async fn disconnect(&self) -> TransportResult<()> {
        let mut stream_guard = self.stream.lock().await;
        if let Some(mut stream) = stream_guard.take() {
            stream
                .close(None)
                .await
                .map_err(|e| TransportError::ConnectionLost(e.to_string()))?;
        }
        Ok(())
    }

    async fn send(&self, message: TransportMessage) -> TransportResult<()> {
        let mut stream_guard = self.stream.lock().await;
        if let Some(ref mut stream) = *stream_guard {
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

    async fn receive(&self) -> TransportResult<Option<TransportMessage>> {
        let mut stream_guard = self.stream.lock().await;
        if let Some(ref mut stream) = *stream_guard {
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
