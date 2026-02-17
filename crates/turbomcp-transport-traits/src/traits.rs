//! Core transport traits.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use futures::{Sink, Stream};

use crate::error::{TransportError, TransportResult};
use crate::message::TransportMessage;
use crate::metrics::TransportMetrics;
use crate::types::{TransportCapabilities, TransportConfig, TransportState, TransportType};

/// The core trait for all transport implementations.
///
/// This trait defines the essential, asynchronous operations for a message-based
/// communication channel, such as connecting, disconnecting, sending, and receiving.
pub trait Transport: Send + Sync + std::fmt::Debug {
    /// Returns the type of this transport.
    fn transport_type(&self) -> TransportType;

    /// Returns the capabilities of this transport.
    fn capabilities(&self) -> &TransportCapabilities;

    /// Returns the current state of the transport.
    fn state(&self) -> Pin<Box<dyn Future<Output = TransportState> + Send + '_>>;

    /// Establishes a connection to the remote endpoint.
    fn connect(&self) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>>;

    /// Closes the connection to the remote endpoint.
    fn disconnect(&self) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>>;

    /// Sends a single message over the transport.
    fn send(
        &self,
        message: TransportMessage,
    ) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>>;

    /// Receives a single message from the transport in a non-blocking way.
    fn receive(
        &self,
    ) -> Pin<Box<dyn Future<Output = TransportResult<Option<TransportMessage>>> + Send + '_>>;

    /// Returns a snapshot of the transport's current performance metrics.
    fn metrics(&self) -> Pin<Box<dyn Future<Output = TransportMetrics> + Send + '_>>;

    /// Returns `true` if the transport is currently in the `Connected` state.
    fn is_connected(&self) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        Box::pin(async move { matches!(self.state().await, TransportState::Connected) })
    }

    /// Returns the endpoint address or identifier for this transport, if applicable.
    fn endpoint(&self) -> Option<String> {
        None
    }

    /// Applies a new configuration to the transport.
    fn configure(
        &self,
        config: TransportConfig,
    ) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
        Box::pin(async move {
            let _ = config;
            Ok(())
        })
    }
}

/// A trait for transports that support full-duplex, bidirectional communication.
///
/// This extends the base `Transport` trait with the ability to send a request and
/// await a correlated response.
pub trait BidirectionalTransport: Transport {
    /// Sends a request message and waits for a corresponding response.
    fn send_request(
        &self,
        message: TransportMessage,
        timeout: Option<Duration>,
    ) -> Pin<Box<dyn Future<Output = TransportResult<TransportMessage>> + Send + '_>>;

    /// Starts tracking a request-response correlation.
    fn start_correlation(
        &self,
        correlation_id: String,
    ) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>>;

    /// Stops tracking a request-response correlation.
    fn stop_correlation(
        &self,
        correlation_id: &str,
    ) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>>;
}

/// A trait for transports that support streaming data.
pub trait StreamingTransport: Transport {
    /// The type of the stream used for sending messages.
    type SendStream: Stream<Item = TransportResult<TransportMessage>> + Send + Unpin;

    /// The type of the sink used for receiving messages.
    type ReceiveStream: Sink<TransportMessage, Error = TransportError> + Send + Unpin;

    /// Returns a stream for sending messages.
    fn send_stream(
        &self,
    ) -> Pin<Box<dyn Future<Output = TransportResult<Self::SendStream>> + Send + '_>>;

    /// Returns a sink for receiving messages.
    fn receive_stream(
        &self,
    ) -> Pin<Box<dyn Future<Output = TransportResult<Self::ReceiveStream>> + Send + '_>>;
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
