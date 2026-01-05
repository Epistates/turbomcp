//! Transport event types.

use tokio::sync::mpsc;
use turbomcp_protocol::MessageId;

use crate::error::TransportError;
use crate::metrics::TransportMetrics;
use crate::types::TransportType;

/// Represents events that occur within a transport's lifecycle.
#[derive(Debug, Clone)]
pub enum TransportEvent {
    /// A new connection has been established.
    Connected {
        /// The type of the transport that connected.
        transport_type: TransportType,
        /// The endpoint of the connection.
        endpoint: String,
    },

    /// A connection has been lost.
    Disconnected {
        /// The type of the transport that disconnected.
        transport_type: TransportType,
        /// The endpoint of the connection.
        endpoint: String,
        /// An optional reason for the disconnection.
        reason: Option<String>,
    },

    /// A message has been successfully sent.
    MessageSent {
        /// The ID of the sent message.
        message_id: MessageId,
        /// The size of the sent message in bytes.
        size: usize,
    },

    /// A message has been successfully received.
    MessageReceived {
        /// The ID of the received message.
        message_id: MessageId,
        /// The size of the received message in bytes.
        size: usize,
    },

    /// An error has occurred in the transport.
    Error {
        /// The error that occurred.
        error: TransportError,
        /// Optional additional context about the error.
        context: Option<String>,
    },

    /// The transport's metrics have been updated.
    MetricsUpdated {
        /// The updated metrics snapshot.
        metrics: TransportMetrics,
    },
}

/// An emitter for broadcasting `TransportEvent`s to listeners.
#[derive(Debug, Clone)]
pub struct TransportEventEmitter {
    sender: mpsc::Sender<TransportEvent>,
}

impl TransportEventEmitter {
    /// Creates a new event emitter and a corresponding receiver.
    #[must_use]
    pub fn new() -> (Self, mpsc::Receiver<TransportEvent>) {
        let (sender, receiver) = mpsc::channel(500);
        (Self { sender }, receiver)
    }

    /// Emits an event, dropping it if the channel is full to avoid blocking.
    pub fn emit(&self, event: TransportEvent) {
        // Use try_send for non-blocking event emission.
        let _ = self.sender.try_send(event);
    }

    /// Emits a `Connected` event.
    pub fn emit_connected(&self, transport_type: TransportType, endpoint: String) {
        self.emit(TransportEvent::Connected {
            transport_type,
            endpoint,
        });
    }

    /// Emits a `Disconnected` event.
    pub fn emit_disconnected(
        &self,
        transport_type: TransportType,
        endpoint: String,
        reason: Option<String>,
    ) {
        self.emit(TransportEvent::Disconnected {
            transport_type,
            endpoint,
            reason,
        });
    }

    /// Emits a `MessageSent` event.
    pub fn emit_message_sent(&self, message_id: MessageId, size: usize) {
        self.emit(TransportEvent::MessageSent { message_id, size });
    }

    /// Emits a `MessageReceived` event.
    pub fn emit_message_received(&self, message_id: MessageId, size: usize) {
        self.emit(TransportEvent::MessageReceived { message_id, size });
    }

    /// Emits an `Error` event.
    pub fn emit_error(&self, error: TransportError, context: Option<String>) {
        self.emit(TransportEvent::Error { error, context });
    }

    /// Emits a `MetricsUpdated` event.
    pub fn emit_metrics_updated(&self, metrics: TransportMetrics) {
        self.emit(TransportEvent::MetricsUpdated { metrics });
    }
}

impl Default for TransportEventEmitter {
    fn default() -> Self {
        Self::new().0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_transport_event_emitter() {
        let (emitter, mut receiver) = TransportEventEmitter::new();

        emitter.emit_connected(TransportType::Stdio, "stdio://".to_string());

        let event = receiver.recv().await.unwrap();
        match event {
            TransportEvent::Connected {
                transport_type,
                endpoint,
            } => {
                assert_eq!(transport_type, TransportType::Stdio);
                assert_eq!(endpoint, "stdio://");
            }
            _ => panic!("Unexpected event variant"),
        }
    }
}
