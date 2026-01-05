//! Core transport traits.

use std::time::Duration;

use async_trait::async_trait;
use futures::{Sink, Stream};

use crate::error::{TransportError, TransportResult};
use crate::message::TransportMessage;
use crate::metrics::TransportMetrics;
use crate::types::{TransportCapabilities, TransportConfig, TransportState, TransportType};

/// The core trait for all transport implementations.
///
/// This trait defines the essential, asynchronous operations for a message-based
/// communication channel, such as connecting, disconnecting, sending, and receiving.
#[async_trait]
pub trait Transport: Send + Sync + std::fmt::Debug {
    /// Returns the type of this transport.
    fn transport_type(&self) -> TransportType;

    /// Returns the capabilities of this transport.
    fn capabilities(&self) -> &TransportCapabilities;

    /// Returns the current state of the transport.
    async fn state(&self) -> TransportState;

    /// Establishes a connection to the remote endpoint.
    async fn connect(&self) -> TransportResult<()>;

    /// Closes the connection to the remote endpoint.
    async fn disconnect(&self) -> TransportResult<()>;

    /// Sends a single message over the transport.
    async fn send(&self, message: TransportMessage) -> TransportResult<()>;

    /// Receives a single message from the transport in a non-blocking way.
    async fn receive(&self) -> TransportResult<Option<TransportMessage>>;

    /// Returns a snapshot of the transport's current performance metrics.
    async fn metrics(&self) -> TransportMetrics;

    /// Returns `true` if the transport is currently in the `Connected` state.
    async fn is_connected(&self) -> bool {
        matches!(self.state().await, TransportState::Connected)
    }

    /// Returns the endpoint address or identifier for this transport, if applicable.
    fn endpoint(&self) -> Option<String> {
        None
    }

    /// Applies a new configuration to the transport.
    async fn configure(&self, config: TransportConfig) -> TransportResult<()> {
        let _ = config;
        Ok(())
    }
}

/// A trait for transports that support full-duplex, bidirectional communication.
///
/// This extends the base `Transport` trait with the ability to send a request and
/// await a correlated response.
#[async_trait]
pub trait BidirectionalTransport: Transport {
    /// Sends a request message and waits for a corresponding response.
    async fn send_request(
        &self,
        message: TransportMessage,
        timeout: Option<Duration>,
    ) -> TransportResult<TransportMessage>;

    /// Starts tracking a request-response correlation.
    async fn start_correlation(&self, correlation_id: String) -> TransportResult<()>;

    /// Stops tracking a request-response correlation.
    async fn stop_correlation(&self, correlation_id: &str) -> TransportResult<()>;
}

/// A trait for transports that support streaming data.
#[async_trait]
pub trait StreamingTransport: Transport {
    /// The type of the stream used for sending messages.
    type SendStream: Stream<Item = TransportResult<TransportMessage>> + Send + Unpin;

    /// The type of the sink used for receiving messages.
    type ReceiveStream: Sink<TransportMessage, Error = TransportError> + Send + Unpin;

    /// Returns a stream for sending messages.
    async fn send_stream(&self) -> TransportResult<Self::SendStream>;

    /// Returns a sink for receiving messages.
    async fn receive_stream(&self) -> TransportResult<Self::ReceiveStream>;
}

/// A factory for creating instances of a specific transport type.
pub trait TransportFactory: Send + Sync + std::fmt::Debug {
    /// Returns the type of transport this factory creates.
    fn transport_type(&self) -> TransportType;

    /// Creates a new transport instance with the given configuration.
    fn create(&self, config: TransportConfig) -> TransportResult<Box<dyn Transport>>;

    /// Returns `true` if this transport is available on the current system.
    fn is_available(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test that traits can be used as trait objects
    fn _test_transport_object(_t: &dyn Transport) {}
    fn _test_bidirectional_object(_t: &dyn BidirectionalTransport) {}
    fn _test_factory_object(_t: &dyn TransportFactory) {}
}
