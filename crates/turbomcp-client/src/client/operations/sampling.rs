//! Sampling operations for MCP client
//!
//! This module provides sampling capability management for LLM operations.
//! Sampling allows the MCP server to request the client to perform LLM
//! inference when the server needs language model capabilities.
//!
//! The client's role in sampling is to:
//! 1. Register handlers for sampling/createMessage requests
//! 2. Advertise sampling capabilities during initialization
//! 3. Process server-initiated sampling requests (handled in core message routing)

use crate::sampling::SamplingHandler;
use std::collections::HashMap;
use std::sync::Arc;
use turbomcp_protocol::types::SamplingCapabilities;

impl<T: turbomcp_transport::Transport + 'static> super::super::core::Client<T> {
    /// Set the sampling handler for processing server-initiated sampling requests
    ///
    /// Registers a handler that can process LLM sampling requests from the server.
    /// When a handler is set, the client will advertise sampling capabilities
    /// during initialization, allowing the server to request LLM operations.
    ///
    /// # Arguments
    ///
    /// * `handler` - The handler implementation for sampling requests
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::{Client, sampling::SamplingHandler};
    /// use turbomcp_transport::stdio::StdioTransport;
    /// use turbomcp_protocol::types::{CreateMessageRequest, CreateMessageResult};
    /// use std::sync::Arc;
    /// use std::future::Future;
    /// use std::pin::Pin;
    ///
    /// #[derive(Debug)]
    /// struct ExampleHandler;
    ///
    /// impl SamplingHandler for ExampleHandler {
    ///     fn handle_create_message(
    ///         &self,
    ///         _request_id: String,
    ///         _request: CreateMessageRequest,
    ///     ) -> Pin<Box<dyn Future<Output = Result<CreateMessageResult, Box<dyn std::error::Error + Send + Sync>>> + Send + '_>> {
    ///         Box::pin(async move {
    ///             // Handle sampling request (use request_id for tracking/correlation)
    ///             todo!("Implement sampling logic")
    ///         })
    ///     }
    /// }
    ///
    /// let mut client = Client::new(StdioTransport::new());
    /// client.set_sampling_handler(Arc::new(ExampleHandler));
    /// ```
    pub fn set_sampling_handler(&self, handler: Arc<dyn SamplingHandler>) {
        *self.inner.sampling_handler.lock() = Some(handler);
    }

    /// Check if sampling is enabled
    ///
    /// Returns true if a sampling handler has been configured and sampling
    /// capabilities are enabled.
    #[must_use]
    pub fn has_sampling_handler(&self) -> bool {
        self.inner.sampling_handler.lock().is_some()
    }

    /// Remove the sampling handler
    ///
    /// Disables sampling capabilities and removes the handler. The client
    /// will no longer advertise sampling support to servers.
    pub fn remove_sampling_handler(&self) {
        *self.inner.sampling_handler.lock() = None;
    }

    /// Declare that this client can run tool loops during sampling.
    ///
    /// Advertises `sampling: {"tools": {}}` at initialization. A server
    /// **MUST NOT** send `tools` or `toolChoice` on `sampling/createMessage`
    /// unless the client declared this, so without it a tool-augmented server
    /// is obliged to fall back to plain sampling — which means your
    /// [`SamplingHandler`] will never see a tool loop no matter what it
    /// supports. New in MCP 2025-11-25.
    ///
    /// Only call this if the handler actually executes tool calls and returns
    /// `tool_use` content; declaring it otherwise invites requests you cannot
    /// answer.
    pub fn enable_sampling_tools(&self) {
        self.inner.sampling_capabilities.lock().tools = Some(HashMap::new());
    }

    /// Declare that this client can gather MCP context for sampling.
    ///
    /// Advertises `sampling: {"context": {}}`, which is what lets a server ask
    /// for `includeContext: "thisServer"` or `"allServers"`.
    ///
    /// Those two values are **soft-deprecated** in 2025-11-25 and servers are
    /// told to use them only against a client that declared this capability.
    /// New code should prefer passing the context it wants in `messages`.
    pub fn enable_sampling_context(&self) {
        self.inner.sampling_capabilities.lock().context = Some(HashMap::new());
    }

    /// Get sampling capabilities for initialization
    ///
    /// Returns the sampling capabilities to be sent during client initialization
    /// if sampling is enabled. A client with a handler but no opt-ins declares
    /// `{}`, which is the correct — and spec-required — statement that it
    /// answers plain `sampling/createMessage` and nothing more.
    pub(crate) fn get_sampling_capabilities(&self) -> Option<SamplingCapabilities> {
        if self.inner.sampling_handler.lock().is_some() {
            Some(self.inner.sampling_capabilities.lock().clone())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::core::Client;
    use super::*;
    use std::collections::VecDeque;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Mutex;
    use turbomcp_protocol::MessageId;
    use turbomcp_protocol::types::{CreateMessageRequest, CreateMessageResult};
    use turbomcp_transport::{
        Transport, TransportCapabilities, TransportError, TransportMessage, TransportMetrics,
        TransportResult, TransportState, TransportType,
    };

    /// Answers `initialize` and keeps the request so the declared capabilities
    /// can be read off the wire rather than out of the struct that built them.
    #[derive(Debug, Default)]
    struct HandshakeTransport {
        capabilities: TransportCapabilities,
        sent: Mutex<Vec<serde_json::Value>>,
        responses: Mutex<VecDeque<TransportMessage>>,
    }

    impl HandshakeTransport {
        fn declared_capabilities(&self) -> serde_json::Value {
            self.sent.lock().expect("sent queue poisoned")[0]["params"]["capabilities"].clone()
        }
    }

    impl Transport for HandshakeTransport {
        fn transport_type(&self) -> TransportType {
            TransportType::Stdio
        }

        fn capabilities(&self) -> &TransportCapabilities {
            &self.capabilities
        }

        fn state(&self) -> Pin<Box<dyn Future<Output = TransportState> + Send + '_>> {
            Box::pin(async { TransportState::Connected })
        }

        fn connect(&self) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }

        fn disconnect(&self) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }

        fn send(
            &self,
            message: TransportMessage,
        ) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
            let request: serde_json::Value = match serde_json::from_slice(&message.payload) {
                Ok(request) => request,
                Err(e) => {
                    return Box::pin(async move {
                        Err(TransportError::SerializationFailed(e.to_string()))
                    });
                }
            };
            self.sent
                .lock()
                .expect("sent queue poisoned")
                .push(request.clone());

            // Notifications take no reply.
            if request.get("id").is_none() {
                return Box::pin(async { Ok(()) });
            }

            let response = serde_json::json!({
                "jsonrpc": "2.0",
                "id": request["id"].clone(),
                "result": {
                    "protocolVersion": turbomcp_protocol::PROTOCOL_VERSION,
                    "capabilities": {},
                    "serverInfo": { "name": "test", "version": "1.0.0" }
                }
            });
            let payload = serde_json::to_vec(&response).expect("response serializes");
            self.responses
                .lock()
                .expect("response queue poisoned")
                .push_back(TransportMessage::new(
                    MessageId::from("response-1"),
                    payload.into(),
                ));
            Box::pin(async { Ok(()) })
        }

        fn receive(
            &self,
        ) -> Pin<Box<dyn Future<Output = TransportResult<Option<TransportMessage>>> + Send + '_>>
        {
            let response = self
                .responses
                .lock()
                .expect("response queue poisoned")
                .pop_front();
            Box::pin(async move { Ok(response) })
        }

        fn metrics(&self) -> Pin<Box<dyn Future<Output = TransportMetrics> + Send + '_>> {
            Box::pin(async { TransportMetrics::default() })
        }
    }

    #[derive(Debug)]
    struct NoopSampling;

    impl SamplingHandler for NoopSampling {
        fn handle_create_message(
            &self,
            _request_id: String,
            _request: CreateMessageRequest,
        ) -> Pin<
            Box<
                dyn Future<
                        Output = Result<
                            CreateMessageResult,
                            Box<dyn std::error::Error + Send + Sync>,
                        >,
                    > + Send
                    + '_,
            >,
        > {
            Box::pin(async { unreachable!("never invoked in these tests") })
        }
    }

    /// A handler alone declares `sampling: {}` — the accurate statement that
    /// this client answers plain `sampling/createMessage` and nothing more.
    #[tokio::test]
    async fn a_plain_handler_declares_bare_sampling() {
        let client = Client::new(HandshakeTransport::default());
        client.set_sampling_handler(Arc::new(NoopSampling));
        client.initialize().await.expect("handshake");

        assert_eq!(
            client.inner.protocol.transport().declared_capabilities()["sampling"],
            serde_json::json!({})
        );
    }

    /// Without this opt-in a spec-abiding server **MUST NOT** send `tools`, so
    /// a client that could run tool loops was unable to say so through any
    /// supported API and could never take part in 2025-11-25 tool sampling.
    #[tokio::test]
    async fn enable_sampling_tools_reaches_the_wire() {
        let client = Client::new(HandshakeTransport::default());
        client.set_sampling_handler(Arc::new(NoopSampling));
        client.enable_sampling_tools();
        client.initialize().await.expect("handshake");

        assert_eq!(
            client.inner.protocol.transport().declared_capabilities()["sampling"],
            serde_json::json!({ "tools": {} })
        );
    }

    #[tokio::test]
    async fn enable_sampling_context_reaches_the_wire() {
        let client = Client::new(HandshakeTransport::default());
        client.set_sampling_handler(Arc::new(NoopSampling));
        client.enable_sampling_context();
        client.enable_sampling_tools();
        client.initialize().await.expect("handshake");

        assert_eq!(
            client.inner.protocol.transport().declared_capabilities()["sampling"],
            serde_json::json!({ "context": {}, "tools": {} })
        );
    }

    /// No handler means no `sampling` key at all: declaring a capability the
    /// client cannot serve is worse than declaring nothing.
    #[tokio::test]
    async fn opting_in_without_a_handler_declares_nothing() {
        let client = Client::new(HandshakeTransport::default());
        client.enable_sampling_tools();
        client.initialize().await.expect("handshake");

        assert!(
            client.inner.protocol.transport().declared_capabilities()["sampling"].is_null(),
            "a client with no handler must not advertise sampling"
        );
    }
}
