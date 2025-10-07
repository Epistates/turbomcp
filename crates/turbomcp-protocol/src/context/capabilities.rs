//! Server-to-client communication capabilities for bidirectional MCP communication.
//!
//! This module defines the trait that enables servers to make requests to clients,
//! supporting sampling, elicitation, and roots listing operations.

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::context::RequestContext;
use crate::error::Error;
use crate::types::{
    CreateMessageRequest, CreateMessageResult, ElicitRequest, ElicitResult, ListRootsResult,
};

/// Trait for server-to-client requests (sampling, elicitation, roots)
///
/// This trait provides a type-safe interface for servers to make requests to clients,
/// enabling bidirectional MCP communication patterns. All methods accept a `RequestContext`
/// parameter to enable proper context propagation for tracing, attribution, and auditing.
///
/// ## Design Rationale
///
/// This trait uses typed request/response structures instead of `serde_json::Value` to provide:
/// - **Type safety**: Compile-time validation of request/response structures
/// - **Performance**: Zero-cost abstraction with no intermediate serialization
/// - **Context propagation**: Full support for distributed tracing and user attribution
/// - **Better errors**: Structured error types instead of generic `Box<dyn Error>`
///
/// ## Breaking Change (v2.0.0)
///
/// This trait was renamed from `ServerCapabilities` to `ServerToClientRequests` and redesigned
/// to fix fundamental architecture issues:
/// - Old: `fn create_message(&self, request: serde_json::Value) -> Result<serde_json::Value, Box<dyn Error>>`
/// - New: `fn create_message(&self, request: CreateMessageRequest, ctx: RequestContext) -> Result<CreateMessageResult, ServerError>`
pub trait ServerToClientRequests: Send + Sync + fmt::Debug {
    /// Send a sampling/createMessage request to the client
    ///
    /// This method allows server tools to request LLM sampling from the client.
    /// The client is responsible for:
    /// - Selecting an appropriate model based on preferences
    /// - Making the LLM API call
    /// - Returning the generated response
    ///
    /// # Arguments
    ///
    /// * `request` - The sampling request with messages, model preferences, and parameters
    /// * `ctx` - Request context for tracing, user attribution, and metadata propagation
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client does not support sampling
    /// - The transport layer fails
    /// - The client returns an error response
    /// - The LLM request fails
    ///
    /// # Example
    ///
    /// ```no_run
    /// use turbomcp_protocol::context::capabilities::ServerToClientRequests;
    /// use turbomcp_protocol::RequestContext;
    /// # use turbomcp_protocol::types::{CreateMessageRequest, SamplingMessage, Role, Content, TextContent};
    ///
    /// async fn example(capabilities: &dyn ServerToClientRequests) {
    ///     let request = CreateMessageRequest {
    ///         messages: vec![SamplingMessage {
    ///             role: Role::User,
    ///             content: Content::Text(TextContent {
    ///                 text: "What is 2+2?".to_string(),
    ///                 annotations: None,
    ///                 meta: None,
    ///             }),
    ///             metadata: None,
    ///         }],
    ///         model_preferences: None,
    ///         system_prompt: None,
    ///         include_context: None,
    ///         temperature: None,
    ///         max_tokens: 100,
    ///         stop_sequences: None,
    ///         _meta: None,
    ///     };
    ///
    ///     let ctx = RequestContext::new();
    ///     # #[allow(unused)]
    ///     let result = capabilities.create_message(request, ctx).await;
    /// }
    /// ```
    fn create_message(
        &self,
        request: CreateMessageRequest,
        ctx: RequestContext,
    ) -> BoxFuture<'_, Result<CreateMessageResult, Error>>;

    /// Send an elicitation request to the client for user input
    ///
    /// This method allows server tools to request structured input from users through
    /// the client's UI. The client is responsible for presenting the elicitation prompt
    /// and collecting the user's response according to the requested schema.
    ///
    /// # Arguments
    ///
    /// * `request` - The elicitation request with prompt and optional schema
    /// * `ctx` - Request context for tracing, user attribution, and metadata propagation
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client does not support elicitation
    /// - The transport layer fails
    /// - The user declines or cancels the request
    /// - The client returns an error response
    fn elicit(
        &self,
        request: ElicitRequest,
        ctx: RequestContext,
    ) -> BoxFuture<'_, Result<ElicitResult, Error>>;

    /// List client's root capabilities
    ///
    /// This method allows servers to discover which directories or files the client
    /// has granted access to. Roots define the filesystem boundaries for resource access.
    ///
    /// # Arguments
    ///
    /// * `ctx` - Request context for tracing, user attribution, and metadata propagation
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client does not support roots
    /// - The transport layer fails
    /// - The client returns an error response
    fn list_roots(
        &self,
        ctx: RequestContext,
    ) -> BoxFuture<'_, Result<ListRootsResult, Error>>;
}

/// Communication direction for bidirectional requests
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommunicationDirection {
    /// Client to server
    ClientToServer,
    /// Server to client
    ServerToClient,
}

/// Communication initiator for tracking request origins
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommunicationInitiator {
    /// Client initiated the request
    Client,
    /// Server initiated the request
    Server,
}

/// Types of server-initiated requests
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServerInitiatedType {
    /// Sampling/message creation request
    Sampling,
    /// Elicitation request for user input
    Elicitation,
    /// Roots listing request
    Roots,
    /// Ping/health check request
    Ping,
}

/// Origin of a ping request
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PingOrigin {
    /// Client initiated ping
    Client,
    /// Server initiated ping
    Server,
}
