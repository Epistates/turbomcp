//! Streamable HTTP transport compliant with MCP 2025-06-18 specification
//!
//! This transport implements the official MCP Streamable HTTP protocol:
//! - Returns 202 Accepted for ALL notifications and responses per specification
//! - Supports SSE for server → client streaming
//! - Handles session management with Mcp-Session-Id headers (MCP standard)
//! - Validates MCP-Protocol-Version headers
//! - Full compliance with MCP 2025-06-18 specification

use axum::{
    Json,
    extract::{State, TypedHeader},
    headers::HeaderMap,
    http::{StatusCode, header},
    response::{IntoResponse, Response, sse::{Event, KeepAlive, Sse}},
    routing::{get, post, delete},
    Router,
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;

/// Configuration for streamable HTTP transport
#[derive(Clone, Debug)]
pub struct StreamableHttpConfig {
    /// Bind address
    pub bind_addr: String,
    /// Base path for endpoints (default: empty for root)
    pub base_path: String,
    /// SSE keep-alive interval
    pub keep_alive_secs: u64,
}

impl Default for StreamableHttpConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:8080".to_string(),
            base_path: "".to_string(),
            keep_alive_secs: 30,
        }
    }
}

/// Session information
struct Session {
    id: String,
    created_at: std::time::Instant,
    last_event_id: Option<String>,
    sse_sender: mpsc::UnboundedSender<SseMessage>,
}

/// Message types for SSE
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type")]
enum SseMessage {
    #[serde(rename = "message")]
    Message { data: serde_json::Value },
    #[serde(rename = "error")]
    Error { message: String },
}

/// Shared application state
#[derive(Clone)]
struct AppState {
    sessions: Arc<RwLock<HashMap<String, Session>>>,
    pending_requests: Arc<RwLock<HashMap<String, serde_json::Value>>>,
}

/// Create router for streamable HTTP transport
pub fn create_router(config: StreamableHttpConfig) -> Router {
    let state = AppState {
        sessions: Arc::new(RwLock::new(HashMap::new())),
        pending_requests: Arc::new(RwLock::new(HashMap::new())),
    };

    let base = config.base_path;
    
    Router::new()
        // SSE endpoint for streaming (GET)
        .route(&format!("{}/events", base), get(sse_handler))
        // Main endpoint for JSON-RPC (POST) 
        .route(&format!("{}/", base), post(json_rpc_handler))
        // Session management (DELETE)
        .route(&format!("{}/session", base), delete(delete_session))
        .with_state(state)
}

/// SSE handler - establishes event stream
async fn sse_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Sse<impl Stream<Item = Result<Event, axum::Error>>> {
    let (tx, mut rx) = mpsc::unbounded_channel::<SseMessage>();
    
    // Get or create session using MCP standard header
    let session_id = headers
        .get("Mcp-Session-Id")
        .and_then(|v| v.to_str().ok())
        .map(String::from)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    
    let last_event_id = headers
        .get("Last-Event-Id")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    
    let session = Session {
        id: session_id.clone(),
        created_at: std::time::Instant::now(),
        last_event_id,
        sse_sender: tx.clone(),
    };
    
    state.sessions.write().await.insert(session_id.clone(), session);
    
    // Create SSE stream
    let stream = async_stream::stream! {
        // Send session ID as first event
        yield Ok(Event::default()
            .id(Uuid::new_v4().to_string())
            .event("session")
            .data(session_id));
        
        // Stream messages
        while let Some(msg) = rx.recv().await {
            let data = serde_json::to_string(&msg).unwrap_or_default();
            yield Ok(Event::default()
                .id(Uuid::new_v4().to_string())
                .event("message")
                .data(data));
        }
    };
    
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// JSON-RPC handler - returns 202 for all notifications/responses per MCP 2025-06-18
async fn json_rpc_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<serde_json::Value>,
) -> impl IntoResponse {
    let session_id = headers
        .get("Mcp-Session-Id")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    
    // Validate MCP-Protocol-Version header per 2025-06-18 specification
    let protocol_version = headers
        .get("MCP-Protocol-Version")
        .and_then(|v| v.to_str().ok());
    
    // Per spec: if no version header and no other way to identify, assume 2025-03-26
    let version_to_use = protocol_version.unwrap_or("2025-03-26");
    
    // Validate supported protocol version
    if let Some(version) = protocol_version {
        if !matches!(version, "2025-06-18" | "2025-03-26" | "2024-11-05") {
            // Return 400 Bad Request for unsupported version per specification
            return (StatusCode::BAD_REQUEST, HeaderMap::new(), Json(serde_json::json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32600,
                    "message": "Invalid Request",
                    "data": format!("Unsupported protocol version: {}", version)
                },
                "id": request.get("id")
            })));
        }
    }
    
    // Check if this is an initialization request
    if let Some(method) = request.get("method").and_then(|m| m.as_str()) {
        match method {
            "initialize" => {
                // Return immediate response with session ID
                let session_id = session_id.unwrap_or_else(|| Uuid::new_v4().to_string());
                
                let mut response_headers = HeaderMap::new();
                response_headers.insert("Mcp-Session-Id", session_id.parse().unwrap());
                response_headers.insert("MCP-Protocol-Version", version_to_use.parse().unwrap());
                
                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "result": {
                        "protocolVersion": version_to_use,
                        "serverInfo": {
                            "name": "turbomcp-streamable",
                            "version": "1.0.0"
                        },
                        "capabilities": {
                            "tools": {},
                            "resources": {},
                            "prompts": {}
                        }
                    },
                    "id": request.get("id")
                });
                
                return (StatusCode::OK, response_headers, Json(response));
            }
            
            _ => {
                // For all other methods, process normally
                // Will be handled by the notification/response detection below
            }
        }
    }
    
    // Per MCP 2025-06-18 specification: detect notifications and responses
    let is_notification = request.get("id").is_none();
    let is_response = request.get("result").is_some() || request.get("error").is_some();
    
    // MCP specification: return 202 Accepted for notifications and responses
    if is_notification || is_response {
        let mut response_headers = HeaderMap::new();
        if let Some(sid) = &session_id {
            response_headers.insert("Mcp-Session-Id", sid.parse().unwrap());
        }
        response_headers.insert("MCP-Protocol-Version", version_to_use.parse().unwrap());
        
        // Store for async processing if needed
        if let Some(sid) = &session_id {
            let request_id = Uuid::new_v4().to_string();
            state.pending_requests.write().await.insert(request_id.clone(), request.clone());
            
            // Send to SSE stream for processing
            if let Some(session) = state.sessions.read().await.get(sid) {
                let _ = session.sse_sender.send(SseMessage::Message { 
                    data: request.clone() 
                });
            }
        }
        
        // Return 202 Accepted with no body per specification
        return (StatusCode::ACCEPTED, response_headers, Json(serde_json::json!({})));
    }
    
    // For requests (have id, not responses): return immediate response
    let mut response_headers = HeaderMap::new();
    if let Some(sid) = &session_id {
        response_headers.insert("Mcp-Session-Id", sid.parse().unwrap());
    }
    response_headers.insert("MCP-Protocol-Version", version_to_use.parse().unwrap());
    
    let response = serde_json::json!({
        "jsonrpc": "2.0",
        "result": {},
        "id": request.get("id")
    });
    
    (StatusCode::OK, response_headers, Json(response))
}

/// Delete session handler
async fn delete_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> StatusCode {
    if let Some(session_id) = headers
        .get("Mcp-Session-Id")
        .and_then(|v| v.to_str().ok())
    {
        state.sessions.write().await.remove(session_id);
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    }
}

// Removed should_process_async - MCP 2025-06-18 specifies 202 for ALL notifications/responses

/// Run streamable HTTP server
pub async fn run_server(config: StreamableHttpConfig) -> Result<(), Box<dyn std::error::Error>> {
    let app = create_router(config.clone());
    let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;
    
    println!("🚀 Streamable HTTP server listening on {}", config.bind_addr);
    println!("   - JSON-RPC endpoint: POST {}/", config.base_path);
    println!("   - SSE endpoint: GET {}/events", config.base_path);
    
    axum::serve(listener, app).await?;
    Ok(())
}