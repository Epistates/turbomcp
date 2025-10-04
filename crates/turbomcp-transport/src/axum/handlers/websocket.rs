//! WebSocket handler for bidirectional MCP communication

use axum::{
    extract::{Extension, Query, State, WebSocketUpgrade, ws::WebSocket},
    response::Response,
};
use futures::{SinkExt, StreamExt};
use tracing::{error, info, trace};

use crate::axum::service::McpAppState;
use crate::axum::types::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, WebSocketQuery};
use crate::tower::SessionInfo;

/// WebSocket handler for upgrade requests
pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(app_state): State<McpAppState>,
    Query(_query): Query<WebSocketQuery>,
    Extension(session): Extension<SessionInfo>,
) -> Response {
    info!("WebSocket upgrade requested for session: {}", session.id);

    ws.on_upgrade(move |socket| handle_websocket(socket, app_state, session))
}

/// Handle WebSocket connection after upgrade
async fn handle_websocket(socket: WebSocket, app_state: McpAppState, session: SessionInfo) {
    let (mut sender, mut receiver) = socket.split();

    info!("WebSocket connected for session: {}", session.id);

    // Send welcome message
    let welcome = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "connected",
        "params": {
            "session_id": session.id,
            "capabilities": app_state.get_capabilities()
        }
    });

    if let Err(e) = sender
        .send(axum::extract::ws::Message::Text(welcome.to_string().into()))
        .await
    {
        error!("Failed to send WebSocket welcome message: {}", e);
        return;
    }

    // Handle incoming messages
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(axum::extract::ws::Message::Text(text)) => {
                trace!("WebSocket received text: {}", text);

                // Parse JSON-RPC request
                match serde_json::from_str::<JsonRpcRequest>(&text) {
                    Ok(request) => {
                        let service_request = serde_json::json!({
                            "jsonrpc": request.jsonrpc,
                            "id": request.id,
                            "method": request.method,
                            "params": request.params
                        });

                        // Process through MCP service using AppState helper
                        match app_state.process_request(service_request, &session).await {
                            Ok(result) => {
                                let response = JsonRpcResponse {
                                    jsonrpc: "2.0".to_string(),
                                    id: request.id,
                                    result: Some(result),
                                    error: None,
                                };

                                let response_text =
                                    serde_json::to_string(&response).unwrap_or_default();
                                if let Err(e) = sender
                                    .send(axum::extract::ws::Message::Text(response_text.into()))
                                    .await
                                {
                                    error!("Failed to send WebSocket response: {}", e);
                                    break;
                                }
                            }
                            Err(e) => {
                                error!("WebSocket MCP service error: {}", e);

                                let error_response = JsonRpcResponse {
                                    jsonrpc: "2.0".to_string(),
                                    id: request.id,
                                    result: None,
                                    error: Some(JsonRpcError {
                                        code: -32603,
                                        message: "Internal error".to_string(),
                                        data: Some(serde_json::json!({
                                            "reason": e.to_string()
                                        })),
                                    }),
                                };

                                let error_text =
                                    serde_json::to_string(&error_response).unwrap_or_default();
                                if let Err(e) = sender
                                    .send(axum::extract::ws::Message::Text(error_text.into()))
                                    .await
                                {
                                    error!("Failed to send WebSocket error response: {}", e);
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to parse WebSocket JSON-RPC request: {}", e);

                        let error_response = JsonRpcResponse {
                            jsonrpc: "2.0".to_string(),
                            id: None,
                            result: None,
                            error: Some(JsonRpcError {
                                code: -32700,
                                message: "Parse error".to_string(),
                                data: Some(serde_json::json!({
                                    "reason": e.to_string()
                                })),
                            }),
                        };

                        let error_text = serde_json::to_string(&error_response).unwrap_or_default();
                        if let Err(e) = sender
                            .send(axum::extract::ws::Message::Text(error_text.into()))
                            .await
                        {
                            error!("Failed to send WebSocket parse error: {}", e);
                            break;
                        }
                    }
                }
            }
            Ok(axum::extract::ws::Message::Close(_)) => {
                info!("WebSocket closed for session: {}", session.id);
                break;
            }
            Ok(axum::extract::ws::Message::Ping(data)) => {
                if let Err(e) = sender.send(axum::extract::ws::Message::Pong(data)).await {
                    error!("Failed to send WebSocket pong: {}", e);
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

    info!("WebSocket disconnected for session: {}", session.id);
}
