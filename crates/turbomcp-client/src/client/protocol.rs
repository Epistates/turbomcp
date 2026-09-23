//! Protocol client for JSON-RPC communication
//!
//! This module provides the ProtocolClient which handles the low-level
//! JSON-RPC protocol communication with MCP servers.
//!
//! ## Bidirectional Communication Architecture
//!
//! The ProtocolClient uses a MessageDispatcher to solve the bidirectional
//! communication problem. Instead of directly calling `transport.receive()`,
//! which created race conditions when multiple code paths tried to receive,
//! we now use a centralized message routing layer:
//!
//! ```text
//! ProtocolClient::request()
//!     ↓
//!   1. Register oneshot channel with dispatcher
//!   2. Send request via transport
//!   3. Wait on oneshot channel
//!     ↓
//! MessageDispatcher (background task)
//!     ↓
//!   Continuously reads transport.receive()
//!   Routes responses → oneshot channels
//!   Routes requests → Client handlers
//! ```
//!
//! This ensures there's only ONE consumer of transport.receive(),
//! eliminating the race condition.

use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcVersion};
use turbomcp_protocol::{Error, Result};
use turbomcp_transport::{Transport, TransportConfig, TransportMessage};

use super::dispatcher::MessageDispatcher;

/// JSON-RPC protocol handler for MCP communication
///
/// Handles request/response correlation, serialization, and protocol-level concerns.
/// This is the abstraction layer between raw Transport and high-level Client APIs.
///
/// ## Architecture
///
/// The ProtocolClient now uses a MessageDispatcher to handle bidirectional
/// communication correctly. The dispatcher runs a background task that:
/// - Reads ALL messages from the transport
/// - Routes responses to waiting request() calls
/// - Routes incoming requests to registered handlers
///
/// This eliminates race conditions by centralizing all message routing
/// in a single background task.
#[derive(Debug)]
pub(super) struct ProtocolClient<T: Transport> {
    transport: Arc<T>,
    dispatcher: Arc<MessageDispatcher>,
    next_id: AtomicU64,
    /// Transport configuration for timeout enforcement (v2.2.0+)
    config: TransportConfig,
    /// Elicitation ids this client has been told about and not yet seen
    /// completed — from URL-mode `elicitation/create` requests, and from the
    /// `data.elicitations` of a -32042 error answering one of our requests.
    ///
    /// The spec requires a client to ignore
    /// `notifications/elicitation/complete` for an unknown or
    /// already-completed id — otherwise a server (or anything able to inject a
    /// notification) can drive a client's retry logic with an id it invented.
    /// It lives here rather than on the client because this is the layer
    /// that sees error responses.
    url_elicitations: Mutex<HashSet<String>>,
}

impl<T: Transport + 'static> ProtocolClient<T> {
    /// Create a new protocol client with custom transport configuration
    ///
    /// This allows setting custom timeouts and limits.
    pub(super) fn with_config(transport: T, config: TransportConfig) -> Self {
        let transport = Arc::new(transport);
        let dispatcher = MessageDispatcher::new(transport.clone());

        Self {
            transport,
            dispatcher,
            next_id: AtomicU64::new(1),
            config,
            url_elicitations: Mutex::new(HashSet::new()),
        }
    }

    /// Remember a URL-mode elicitation id so its completion is honoured.
    pub(super) fn track_url_elicitation(&self, elicitation_id: String) {
        self.url_elicitations.lock().insert(elicitation_id);
    }

    /// Track every elicitation id in a -32042 error's `data`.
    ///
    /// A client retries the failed request once these complete, so their
    /// completions have to be honoured just like those of an
    /// `elicitation/create` it received directly.
    fn track_elicitations_in(&self, data: &serde_json::Value) {
        let ids = data
            .get("elicitations")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|e| e.get("elicitationId").and_then(serde_json::Value::as_str));
        let mut tracked = self.url_elicitations.lock();
        for id in ids {
            tracked.insert(id.to_owned());
        }
    }

    /// Consume a URL-mode elicitation id. `false` means the id is unknown or
    /// already completed, and the completion must be ignored.
    pub(super) fn complete_url_elicitation(&self, elicitation_id: &str) -> bool {
        self.url_elicitations.lock().remove(elicitation_id)
    }

    /// Get the message dispatcher for handler registration
    ///
    /// This allows the Client to register request/notification handlers
    /// with the dispatcher.
    pub(super) fn dispatcher(&self) -> &Arc<MessageDispatcher> {
        &self.dispatcher
    }

    /// Send JSON-RPC request and await typed response
    ///
    /// ## New Architecture (v2.0+)
    ///
    /// Instead of calling `transport.receive()` directly (which created the
    /// race condition), this method now:
    ///
    /// 1. Registers a oneshot channel with the dispatcher BEFORE sending
    /// 2. Sends the request via transport
    /// 3. Waits on the oneshot channel for the response
    ///
    /// The dispatcher's background task receives the response and routes it
    /// to the oneshot channel. This ensures responses always reach the right
    /// request() call, even when the server sends requests (elicitation, etc.)
    /// in between.
    ///
    /// ## Example Flow with Elicitation
    ///
    /// ```text
    /// Client: call_tool("test") → request(id=1)
    ///   1. Register oneshot channel for id=1
    ///   2. Send tools/call request
    ///   3. Wait on channel...
    ///
    /// Server: Sends elicitation/create request (id=2)
    ///   → Dispatcher routes to request handler
    ///   → Client processes elicitation
    ///   → Client sends elicitation response
    ///
    /// Server: Sends tools/call response (id=1)
    ///   → Dispatcher routes to oneshot channel for id=1
    ///   → request() receives response ✓
    /// ```
    pub(super) async fn request<R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<R> {
        // Wrap the entire operation in total timeout (if configured)
        let operation = self.request_inner(method, params);

        if let Some(total_timeout) = self.config.timeouts.total {
            match tokio::time::timeout(total_timeout, operation).await {
                Ok(result) => result,
                Err(_) => {
                    let err = turbomcp_transport::TransportError::TotalTimeout {
                        operation: format!("{}()", method),
                        timeout: total_timeout,
                    };
                    Err(Error::transport(err.to_string()))
                }
            }
        } else {
            operation.await
        }
    }

    /// Inner request implementation without total timeout wrapper
    async fn request_inner<R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<R> {
        // Generate unique request ID
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request_id = turbomcp_protocol::MessageId::from(id.to_string());

        // Build JSON-RPC request
        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: request_id.clone(),
            method: method.to_string(),
            params,
        };

        // Step 1: Register oneshot channel BEFORE sending request via the
        // RAII guard so a mid-flight future drop can't leak the waiter map
        // entry (cancellation-safety).
        let (response_receiver, waiter_guard) = self
            .dispatcher
            .wait_for_response_guarded(request_id.clone());

        // Step 2: Serialize and send request
        let payload = serde_json::to_vec(&request)
            .map_err(|e| Error::internal(format!("Failed to serialize request: {e}")))?;

        let message = TransportMessage::new(
            turbomcp_protocol::MessageId::from(format!("req-{id}")),
            payload.into(),
        );

        // The guard cleans up the waiter if `send` errors out (drop fires
        // when we leave this scope).
        self.transport
            .send(message)
            .await
            .map_err(|e| Error::transport(format!("Transport send failed: {e}")))?;

        // Step 3: Wait for response via oneshot channel with request timeout
        // The dispatcher's background task will send the response when it arrives
        let response = if let Some(request_timeout) = self.config.timeouts.request {
            match tokio::time::timeout(request_timeout, response_receiver).await {
                Ok(Ok(response)) => response,
                Ok(Err(_)) => {
                    return Err(Error::transport("Response channel closed".to_string()));
                }
                Err(_) => {
                    // Best-effort `notifications/cancelled` so a compliant
                    // server can stop in-flight work. Failure to send is
                    // logged and ignored — the local timeout still wins.
                    //
                    // `initialize` is exempt: the cancellation utility says the
                    // initialize request MUST NOT be cancelled. A slow
                    // handshake — cold start, OAuth discovery, a large index —
                    // is exactly when this timeout fires, so without the guard
                    // the client emits a forbidden notification precisely when
                    // it is most likely to be observed.
                    if method != "initialize" {
                        let _ = self
                            .send_cancellation(&request_id, Some("client request timeout"))
                            .await;
                    }
                    let err = turbomcp_transport::TransportError::RequestTimeout {
                        operation: format!("{}()", method),
                        timeout: request_timeout,
                    };
                    return Err(Error::transport(err.to_string()));
                }
            }
        } else {
            response_receiver
                .await
                .map_err(|_| Error::transport("Response channel closed".to_string()))?
        };

        // Response arrived — disarm the guard so it doesn't double-remove.
        waiter_guard.disarm();

        // Handle JSON-RPC errors. `data` is kept: for some errors it is the
        // payload — a -32042 carries the URLs the user has to visit.
        if let Some(error) = response.error() {
            let mut err = Error::from_rpc_code(error.code, &error.message);
            if let Some(data) = &error.data {
                if error.code == turbomcp_protocol::types::URLElicitationRequiredError::ERROR_CODE {
                    self.track_elicitations_in(data);
                }
                err = err.with_data(data.clone());
            }
            return Err(err);
        }

        // Deserialize result
        serde_json::from_value(response.result().unwrap_or_default().clone())
            .map_err(|e| Error::internal(format!("Failed to deserialize response: {e}")))
    }

    /// Send a `notifications/cancelled` notification for the given request id.
    ///
    /// Per MCP 2025-11-25 §Cancellation, when a client abandons an in-flight
    /// request (timeout, future drop, user cancellation) it SHOULD send this
    /// notification so the server can stop work. The `initialize` request
    /// MUST NOT be cancelled per spec — callers gate that themselves.
    pub(super) async fn send_cancellation(
        &self,
        request_id: &turbomcp_protocol::MessageId,
        reason: Option<&str>,
    ) -> Result<()> {
        let mut params = serde_json::Map::new();
        params.insert(
            "requestId".to_string(),
            serde_json::to_value(request_id)
                .map_err(|e| Error::internal(format!("Failed to serialize requestId: {e}")))?,
        );
        if let Some(reason) = reason {
            params.insert(
                "reason".to_string(),
                serde_json::Value::String(reason.into()),
            );
        }
        self.dispatcher.remove_response_waiter(request_id);
        self.notify(
            "notifications/cancelled",
            Some(serde_json::Value::Object(params)),
        )
        .await
    }

    /// Send JSON-RPC notification (no response expected)
    pub(super) async fn notify(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<()> {
        // `params` is omitted rather than sent as `null`: JSON-RPC requires it
        // to be a structured value when present, and the TypeScript SDK's
        // schema rejects `"params": null` outright — which, for
        // `notifications/initialized`, fails the whole handshake.
        let mut request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
        });
        if let Some(params) = params {
            request["params"] = params;
        }

        let payload = serde_json::to_vec(&request)
            .map_err(|e| Error::internal(format!("Failed to serialize notification: {e}")))?;

        let message = TransportMessage::new(
            turbomcp_protocol::MessageId::from("notification"),
            payload.into(),
        );

        self.transport
            .send(message)
            .await
            .map_err(|e| Error::transport(format!("Transport send failed: {e}")))
    }

    /// Get transport reference
    ///
    /// Returns an Arc reference to the transport, allowing it to be shared
    /// with other components (like the message dispatcher).
    pub(super) fn transport(&self) -> &Arc<T> {
        &self.transport
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    use turbomcp_transport::{
        TransportCapabilities, TransportConfig, TransportError, TransportMetrics, TransportResult,
        TransportState, TransportType,
    };

    #[derive(Debug)]
    struct MockTransport {
        capabilities: TransportCapabilities,
        fail_send: AtomicBool,
    }

    impl MockTransport {
        fn ok() -> Self {
            Self {
                capabilities: TransportCapabilities::default(),
                fail_send: AtomicBool::new(false),
            }
        }

        fn fail_send() -> Self {
            Self {
                capabilities: TransportCapabilities::default(),
                fail_send: AtomicBool::new(true),
            }
        }
    }

    impl Transport for MockTransport {
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
            _message: TransportMessage,
        ) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
            let fail = self.fail_send.load(Ordering::Relaxed);
            Box::pin(async move {
                if fail {
                    Err(TransportError::SendFailed("send failed".to_string()))
                } else {
                    Ok(())
                }
            })
        }

        fn receive(
            &self,
        ) -> Pin<Box<dyn Future<Output = TransportResult<Option<TransportMessage>>> + Send + '_>>
        {
            Box::pin(async { Ok(None) })
        }

        fn metrics(&self) -> Pin<Box<dyn Future<Output = TransportMetrics> + Send + '_>> {
            Box::pin(async { TransportMetrics::default() })
        }

        fn configure(
            &self,
            _config: TransportConfig,
        ) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }
    }

    #[tokio::test]
    async fn test_request_timeout_cleans_up_waiter() {
        let config = TransportConfig {
            timeouts: turbomcp_transport::config::TimeoutConfig {
                request: Some(Duration::from_millis(10)),
                total: Some(Duration::from_millis(25)),
                ..Default::default()
            },
            ..Default::default()
        };
        let client = ProtocolClient::with_config(MockTransport::ok(), config);

        let result: Result<serde_json::Value> = client.request("tools/list", None).await;
        assert!(result.is_err());
        assert_eq!(client.dispatcher.response_waiter_count(), 0);

        client.dispatcher.shutdown();
    }

    #[tokio::test]
    async fn test_send_failure_cleans_up_waiter() {
        let client =
            ProtocolClient::with_config(MockTransport::fail_send(), TransportConfig::default());

        let result: Result<serde_json::Value> = client.request("tools/list", None).await;
        assert!(result.is_err());
        assert_eq!(client.dispatcher.response_waiter_count(), 0);

        client.dispatcher.shutdown();
    }
}
