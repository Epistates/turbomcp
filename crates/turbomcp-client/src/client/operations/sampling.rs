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
    /// use async_trait::async_trait;
    /// use std::sync::Arc;
    ///
    /// #[derive(Debug)]
    /// struct ExampleHandler;
    ///
    /// #[async_trait]
    /// impl SamplingHandler for ExampleHandler {
    ///     async fn handle_create_message(
    ///         &self,
    ///         _request: CreateMessageRequest,
    ///     ) -> Result<CreateMessageResult, Box<dyn std::error::Error + Send + Sync>> {
    ///         // Handle sampling request
    ///         todo!("Implement sampling logic")
    ///     }
    /// }
    ///
    /// let mut client = Client::new(StdioTransport::new());
    /// client.set_sampling_handler(Arc::new(ExampleHandler));
    /// ```
    pub fn set_sampling_handler(&self, handler: Arc<dyn SamplingHandler>) {
        *self
            .inner
            .sampling_handler
            .lock()
            .expect("sampling_handler mutex poisoned") = Some(handler);
    }

    /// Check if sampling is enabled
    ///
    /// Returns true if a sampling handler has been configured and sampling
    /// capabilities are enabled.
    pub fn has_sampling_handler(&self) -> bool {
        self.inner
            .sampling_handler
            .lock()
            .expect("sampling_handler mutex poisoned")
            .is_some()
    }

    /// Remove the sampling handler
    ///
    /// Disables sampling capabilities and removes the handler. The client
    /// will no longer advertise sampling support to servers.
    pub fn remove_sampling_handler(&self) {
        *self
            .inner
            .sampling_handler
            .lock()
            .expect("sampling_handler mutex poisoned") = None;
    }

    /// Get sampling capabilities for initialization
    ///
    /// Returns the sampling capabilities to be sent during client initialization
    /// if sampling is enabled.
    pub(crate) fn get_sampling_capabilities(&self) -> Option<SamplingCapabilities> {
        if self
            .inner
            .sampling_handler
            .lock()
            .expect("sampling_handler mutex poisoned")
            .is_some()
        {
            Some(SamplingCapabilities)
        } else {
            None
        }
    }
}
