//! Factory-based WebSocket handler for advanced per-connection customization
//!
//! This module provides a WebSocket handler that uses a factory pattern to create
//! per-connection handlers. This is used by ServerBuilder to create bidirectional
//! wrappers with connection-specific dispatchers.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Extension, Query, State, WebSocketUpgrade, ws::WebSocket},
    response::Response,
};
use futures::{SinkExt, StreamExt};
use tokio::sync::{Mutex, mpsc};
use tracing::{error, info, trace};

use turbomcp_protocol::JsonRpcHandler;

use crate::axum::types::WebSocketQuery;
use crate::axum::websocket_bidirectional::{
    WebSocketDispatcher, handle_response_correlation, is_response,
};
use crate::tower::SessionInfo;

/// Factory function type for creating per-connection handlers
///
/// The factory receives:
/// - `WebSocketDispatcher`: For server→client requests
/// - `Option<HashMap<String, String>>`: Optional HTTP headers from the WebSocket upgrade request
/// - `Option<String>`: Optional tenant ID extracted from request (multi-tenancy support)
///
/// And returns:
/// - `Arc<dyn JsonRpcHandler>`: Handler for this specific connection
pub type HandlerFactory = Arc<
    dyn Fn(WebSocketDispatcher, Option<HashMap<String, String>>, Option<String>) -> Arc<dyn JsonRpcHandler>
        + Send
        + Sync,
>;

/// Application state for factory-based WebSocket handler
#[derive(Clone)]
pub struct WebSocketFactoryState {
    /// Factory for creating per-connection handlers
    pub handler_factory: HandlerFactory,
}

impl std::fmt::Debug for WebSocketFactoryState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebSocketFactoryState")
            .field("handler_factory", &"<factory function>")
            .finish()
    }
}

impl WebSocketFactoryState {
    /// Create new factory state
    pub fn new<F>(factory: F) -> Self
    where
        F: Fn(WebSocketDispatcher, Option<HashMap<String, String>>, Option<String>) -> Arc<dyn JsonRpcHandler>
            + Send
            + Sync
            + 'static,
    {
        Self {
            handler_factory: Arc::new(factory),
        }
    }
}

/// WebSocket handler that uses a factory to create per-connection handlers
///
/// This handler:
/// 1. Creates a WebSocketDispatcher for the connection
/// 2. Calls the factory to create a connection-specific handler
/// 3. Uses that handler for all requests from this connection
///
/// This pattern enables bidirectional MCP support with connection-scoped state.
pub async fn websocket_handler_with_factory(
    ws: WebSocketUpgrade,
    State(factory_state): State<WebSocketFactoryState>,
    Query(_query): Query<WebSocketQuery>,
    Extension(session): Extension<SessionInfo>,
) -> Response {
    info!(
        "WebSocket upgrade requested for session: {} (factory mode)",
        session.id
    );

    // TODO: For multi-tenancy support, tenant_id could be extracted from session metadata
    // or passed via custom headers. For now, WebSocket doesn't extract tenant directly
    // to avoid circular dependencies between turbomcp-transport and turbomcp-server.
    let tenant_id: Option<String> = None;

    ws.on_upgrade(move |socket| handle_websocket_with_factory(socket, factory_state, session, tenant_id))
}

/// Handle WebSocket connection using factory pattern
async fn handle_websocket_with_factory(
    socket: WebSocket,
    factory_state: WebSocketFactoryState,
    session: SessionInfo,
    tenant_id: Option<String>,
) {
    let (ws_sender, ws_receiver) = socket.split();

    info!(
        "WebSocket connected for session: {} (factory mode)",
        session.id
    );

    // Create channels for bidirectional communication
    let (outbound_tx, outbound_rx) = mpsc::unbounded_channel();
    let pending_requests = Arc::new(Mutex::new(HashMap::new()));

    // Create WebSocket dispatcher for server→client requests
    let dispatcher = WebSocketDispatcher::new(outbound_tx.clone(), pending_requests.clone());

    // Extract headers from session metadata
    let headers = if !session.metadata.is_empty() {
        Some(session.metadata.clone())
    } else {
        None
    };

    // Call factory to create connection-specific handler with headers and tenant_id
    let handler = (factory_state.handler_factory)(dispatcher, headers, tenant_id);

    info!("Factory created handler for session: {}", session.id);

    // Spawn send loop (server→client messages)
    let send_task = tokio::spawn(send_loop(ws_sender, outbound_rx));

    // Spawn receive loop (client→server messages + response correlation)
    let session_clone = session.clone();
    let receive_task = tokio::spawn(receive_loop_with_handler(
        ws_receiver,
        handler,
        session_clone,
        outbound_tx,
        pending_requests,
    ));

    // Wait for either task to complete (connection close)
    tokio::select! {
        result = send_task => {
            if let Err(e) = result {
                error!("WebSocket send loop error: {}", e);
            }
            info!("WebSocket send loop terminated for session: {}", session.id);
        }
        result = receive_task => {
            if let Err(e) = result {
                error!("WebSocket receive loop error: {}", e);
            }
            info!("WebSocket receive loop terminated for session: {}", session.id);
        }
    }

    info!("WebSocket disconnected for session: {}", session.id);
}

/// Send loop: forwards messages from channel to WebSocket
async fn send_loop(
    mut sender: futures::stream::SplitSink<WebSocket, axum::extract::ws::Message>,
    mut outbound_rx: mpsc::UnboundedReceiver<axum::extract::ws::Message>,
) {
    while let Some(message) = outbound_rx.recv().await {
        // Send message to buffer
        if let Err(e) = sender.send(message).await {
            error!("Failed to send WebSocket message: {}", e);
            break;
        }

        // Flush buffer to network (CRITICAL for futures::Sink)
        if let Err(e) = sender.flush().await {
            error!("Failed to flush WebSocket message: {}", e);
            break;
        }
    }
    trace!("Send loop exiting");
}

/// Receive loop using factory-created handler
async fn receive_loop_with_handler(
    mut receiver: futures::stream::SplitStream<WebSocket>,
    handler: Arc<dyn JsonRpcHandler>,
    session: SessionInfo,
    outbound_tx: mpsc::UnboundedSender<axum::extract::ws::Message>,
    pending_requests: Arc<
        Mutex<
            HashMap<
                String,
                tokio::sync::oneshot::Sender<turbomcp_protocol::jsonrpc::JsonRpcResponse>,
            >,
        >,
    >,
) {
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(axum::extract::ws::Message::Text(text)) => {
                trace!("WebSocket received text: {} bytes", text.len());

                // Parse JSON
                let value: serde_json::Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(e) => {
                        error!("Failed to parse JSON: {}", e);
                        continue;
                    }
                };

                // Check if this is a response to a server-initiated request
                if is_response(&value) {
                    match serde_json::from_value::<turbomcp_protocol::jsonrpc::JsonRpcResponse>(
                        value.clone(),
                    ) {
                        Ok(response) => {
                            if handle_response_correlation(response, &pending_requests).await {
                                continue; // Response was correlated
                            }
                            // Response not matched - could be unsolicited
                            continue;
                        }
                        Err(e) => {
                            error!("Failed to parse response: {}", e);
                            continue;
                        }
                    }
                }

                // Otherwise, treat as client→server request
                // Inject session metadata
                let mut request_with_metadata = value;
                if let Some(obj) = request_with_metadata.as_object_mut() {
                    // Add session headers
                    if let Ok(headers_json) = serde_json::to_value(&session.metadata) {
                        obj.insert("_mcp_headers".to_string(), headers_json);
                    }
                    // Add transport type
                    obj.insert("_mcp_transport".to_string(), serde_json::json!("websocket"));
                }

                // Process through handler
                let response = handler.handle_request(request_with_metadata).await;

                // Send response
                let response_json = match serde_json::to_string(&response) {
                    Ok(json) => json,
                    Err(e) => {
                        error!("Failed to serialize response: {}", e);
                        continue;
                    }
                };

                if let Err(e) =
                    outbound_tx.send(axum::extract::ws::Message::Text(response_json.into()))
                {
                    error!("Failed to queue WebSocket response: {}", e);
                    break;
                }
            }
            Ok(axum::extract::ws::Message::Close(_)) => {
                info!("WebSocket closed for session: {}", session.id);
                break;
            }
            Ok(axum::extract::ws::Message::Ping(data)) => {
                if let Err(e) = outbound_tx.send(axum::extract::ws::Message::Pong(data)) {
                    error!("Failed to queue WebSocket pong: {}", e);
                    break;
                }
            }
            Err(e) => {
                error!("WebSocket error for session {}: {}", session.id, e);
                break;
            }
            _ => {
                // Ignore other message types (Binary, Pong)
            }
        }
    }
    trace!("Receive loop exiting for session: {}", session.id);
}
