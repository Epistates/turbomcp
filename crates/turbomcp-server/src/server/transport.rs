//! Transport message handling for MCP server
//!
//! Contains the transport-specific message handling logic for processing
//! JSON-RPC messages received through various transport layers.
//!
//! ## Tower Service Integration
//!
//! This module integrates the Tower service stack with transport-layer message handling.
//! All requests flow through the complete middleware pipeline before reaching the router.

use tower::ServiceExt; // For oneshot
use tracing::info_span;

use crate::{ServerError, ServerResult};
use bytes::Bytes;
use http::{Request, Response};
use turbomcp_transport::core::TransportMessageMetadata;
use turbomcp_transport::{Transport, TransportMessage};

use super::core::{McpServer, should_log_for_stdio};

impl McpServer {
    /// Handle transport message through Tower service stack
    ///
    /// This is the world-class Tower integration - all requests flow through
    /// the complete middleware pipeline (timeout, validation, authz, etc.)
    /// before reaching the router.
    /// Used by feature-gated transport methods (http, tcp, websocket, unix)
    #[allow(dead_code)]
    #[tracing::instrument(skip(self, transport, message), fields(
        message_id = ?message.id,
        message_type = "request"
    ))]
    pub(super) async fn handle_transport_message(
        &self,
        transport: &mut dyn Transport,
        message: TransportMessage,
    ) -> ServerResult<()> {
        let request_span = info_span!("tower.service_call");

        // Convert TransportMessage to http::Request<Bytes>
        // The service expects HTTP-style requests for Tower compatibility
        let http_request = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("content-type", "application/json")
            .body(message.payload.clone())
            .map_err(|e| ServerError::Internal(format!("Failed to build HTTP request: {e}")))?;

        // Clone the service to get a mutable instance for the call
        // BoxCloneService is designed for this pattern (Clone but !Sync)
        let service = self.service.clone();

        // Call the service through the complete Tower middleware stack
        // This uses Tower's oneshot helper for single-request services
        let http_response: Response<Bytes> = request_span
            .in_scope(|| service.oneshot(http_request))
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Service call failed");
                e
            })?;

        // Extract response bytes from HTTP response
        let response_bytes = http_response.into_body();

        // Wrap in TransportMessage and send back
        let reply = TransportMessage::with_metadata(
            message.id,
            response_bytes,
            TransportMessageMetadata::with_content_type("application/json"),
        );

        if let Err(e) = transport.send(reply).await {
            tracing::warn!(error = %e, "Failed to send response over transport");
        }

        Ok(())
    }

    /// STDIO-aware message handler that respects MCP protocol logging requirements
    ///
    /// Same as handle_transport_message but with conditional logging for STDIO transport.
    /// STDIO transport must keep stdout clean for JSON-RPC messages per MCP protocol.
    #[tracing::instrument(skip(self, transport, message), fields(
        message_id = ?message.id,
        message_type = "request",
        transport = "stdio"
    ))]
    pub(super) async fn handle_transport_message_stdio_aware(
        &self,
        transport: &mut dyn Transport,
        message: TransportMessage,
    ) -> ServerResult<()> {
        let request_span = info_span!("tower.service_call");

        // Convert TransportMessage to http::Request<Bytes>
        let http_request = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("content-type", "application/json")
            .body(message.payload.clone())
            .map_err(|e| {
                if should_log_for_stdio() {
                    tracing::error!(error = %e, "Failed to build HTTP request");
                }
                ServerError::Internal(format!("Failed to build HTTP request: {e}"))
            })?;

        // Clone the service to get a mutable instance
        let service = self.service.clone();

        // Call the service through the complete Tower middleware stack
        let http_response: Response<Bytes> = request_span
            .in_scope(|| service.oneshot(http_request))
            .await
            .map_err(|e| {
                if should_log_for_stdio() {
                    tracing::error!(error = %e, "Service call failed");
                }
                e
            })?;

        // Extract response bytes from HTTP response
        let response_bytes = http_response.into_body();

        // Wrap in TransportMessage and send back
        let reply = TransportMessage::with_metadata(
            message.id,
            response_bytes,
            TransportMessageMetadata::with_content_type("application/json"),
        );

        if let Err(e) = transport.send(reply).await {
            if should_log_for_stdio() {
                tracing::warn!(error = %e, "Failed to send response over transport");
            }
        }

        Ok(())
    }
}
