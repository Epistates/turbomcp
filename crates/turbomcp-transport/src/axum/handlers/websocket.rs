//! WebSocket handler for bidirectional MCP communication
//!
//! This handler provides full MCP 2025-06-18 bidirectional support, enabling
//! both client→server and server→client requests over WebSocket.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Extension, Query, State, WebSocketUpgrade, ws::WebSocket},
    response::Response,
};
use futures::{SinkExt, StreamExt};
use tokio::sync::{Mutex, mpsc};
use tracing::{error, info, trace, warn};

use crate::axum::service::McpAppState;
use crate::axum::types::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, WebSocketQuery};
use crate::axum::websocket_bidirectional::{
    WebSocketDispatcher, handle_response_correlation, is_response,
};
use crate::tower::SessionInfo;

/// WebSocket handler for upgrade requests
pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<McpAppState>,
    Query(_query): Query<WebSocketQuery>,
    Extension(session): Extension<SessionInfo>,
) -> Response {
    info!("WebSocket upgrade requested for session: {}", session.id);

    ws.on_upgrade(move |socket| handle_websocket_bidirectional(socket, app_state, session))
}

/// Handle WebSocket connection with full bidirectional support
async fn handle_websocket_bidirectional(
    socket: WebSocket,
    app_state: McpAppState,
    session: SessionInfo,
) {
    let (ws_sender, ws_receiver) = socket.split();

    info!("WebSocket connected for session: {}", session.id);

    // Create channels for bidirectional communication
    let (outbound_tx, outbound_rx) = mpsc::unbounded_channel();
    let pending_requests = Arc::new(Mutex::new(HashMap::new()));

    // Create WebSocket dispatcher for server→client requests
    let _dispatcher = WebSocketDispatcher::new(outbound_tx.clone(), pending_requests.clone());

    // TODO: Make dispatcher available to McpService via app_state extension
    // For now, the infrastructure is in place but we need to wire it through McpService

    // Send welcome message
    let welcome = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "connected",
        "params": {
            "session_id": session.id,
            "capabilities": app_state.get_capabilities()
        }
    });

    if let Err(e) = outbound_tx.send(axum::extract::ws::Message::Text(welcome.to_string().into())) {
        error!("Failed to queue WebSocket welcome message: {}", e);
        return;
    }

    // Spawn send loop (server→client messages)
    let send_task = tokio::spawn(send_loop(ws_sender, outbound_rx));

    // Spawn receive loop (client→server messages + response correlation)
    let session_clone = session.clone();
    let receive_task = tokio::spawn(receive_loop(
        ws_receiver,
        app_state,
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
            info!("WebSocket send loop terminated");
        }
        result = receive_task => {
            if let Err(e) = result {
                error!("WebSocket receive loop error: {}", e);
            }
            info!("WebSocket receive loop terminated");
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

/// Receive loop: processes incoming WebSocket messages
///
/// Handles two types of messages:
/// 1. Responses to server-initiated requests (correlate via pending_requests)
/// 2. Client-initiated requests (process through McpService)
async fn receive_loop(
    mut receiver: futures::stream::SplitStream<WebSocket>,
    app_state: McpAppState,
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
                        send_parse_error(&outbound_tx, None).await;
                        continue;
                    }
                };

                // Check if this is a response to a server-initiated request
                if is_response(&value) {
                    // Try to parse as JsonRpcResponse
                    match serde_json::from_value::<turbomcp_protocol::jsonrpc::JsonRpcResponse>(
                        value.clone(),
                    ) {
                        Ok(response) => {
                            if handle_response_correlation(response, &pending_requests).await {
                                // Response was correlated, continue to next message
                                continue;
                            }
                            // Response not matched - could be unsolicited, log and continue
                            warn!("Received uncorrelated response, ignoring");
                            continue;
                        }
                        Err(e) => {
                            error!("Failed to parse response: {}", e);
                            continue;
                        }
                    }
                }

                // Otherwise, treat as client→server request
                match serde_json::from_str::<JsonRpcRequest>(&text) {
                    Ok(request) => {
                        // Process request through MCP service
                        handle_client_request(request, &app_state, &session, &outbound_tx).await;
                    }
                    Err(e) => {
                        error!("Failed to parse WebSocket JSON-RPC request: {}", e);
                        send_parse_error(&outbound_tx, None).await;
                    }
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

/// Handle a client→server request
async fn handle_client_request(
    request: JsonRpcRequest,
    app_state: &McpAppState,
    session: &SessionInfo,
    outbound_tx: &mpsc::UnboundedSender<axum::extract::ws::Message>,
) {
    let method = request.method.clone();
    let request_id = request.id.clone();

    trace!(
        "Handling WebSocket request: method={}, id={:?}",
        method, request_id
    );

    let service_request = serde_json::json!({
        "jsonrpc": request.jsonrpc,
        "id": request.id,
        "method": request.method,
        "params": request.params
    });

    // Process through MCP service
    match app_state.process_request(service_request, session).await {
        Ok(result) => {
            let response = JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request_id,
                result: Some(result),
                error: None,
            };

            send_response(outbound_tx, response).await;
        }
        Err(e) => {
            error!("WebSocket MCP service error: {}", e);

            let error_response = JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request_id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32603,
                    message: "Internal error".to_string(),
                    data: Some(serde_json::json!({
                        "reason": e.to_string()
                    })),
                }),
            };

            send_response(outbound_tx, error_response).await;
        }
    }
}

/// Send a JSON-RPC response
async fn send_response(
    outbound_tx: &mpsc::UnboundedSender<axum::extract::ws::Message>,
    response: JsonRpcResponse,
) {
    let response_text = match serde_json::to_string(&response) {
        Ok(text) => text,
        Err(e) => {
            error!("Failed to serialize response: {}", e);
            return;
        }
    };

    if let Err(e) = outbound_tx.send(axum::extract::ws::Message::Text(response_text.into())) {
        error!("Failed to queue WebSocket response: {}", e);
    }
}

/// Send a parse error response
async fn send_parse_error(
    outbound_tx: &mpsc::UnboundedSender<axum::extract::ws::Message>,
    id: Option<serde_json::Value>,
) {
    let error_response = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: None,
        error: Some(JsonRpcError {
            code: -32700,
            message: "Parse error".to_string(),
            data: None,
        }),
    };

    send_response(outbound_tx, error_response).await;
}
