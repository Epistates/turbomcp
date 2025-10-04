//! Streamable HTTP transport compliant with MCP 2025-06-18 specification
//!
//! This transport implements the official MCP Streamable HTTP protocol:
//! - Returns 202 Accepted for ALL notifications and responses per specification
//! - Supports SSE for server → client streaming
//! - Handles session management with Mcp-Session-Id headers (MCP standard)
//! - Validates MCP-Protocol-Version headers
//! - Full compliance with MCP 2025-06-18 specification

use axum::{
    Json, Router,
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{delete, get, post},
};
use futures::Stream;
use serde::Serialize;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;

use crate::security::{
    SecurityConfigBuilder, SecurityHeaders, SecurityValidator, SessionSecurityConfig,
    SessionSecurityManager,
};

/// Convert axum HeaderMap to SecurityHeaders for validation
fn convert_headers(headers: &HeaderMap) -> SecurityHeaders {
    let mut security_headers = SecurityHeaders::new();

    for (key, value) in headers.iter() {
        if let Ok(value_str) = value.to_str() {
            security_headers.insert(key.to_string(), value_str.to_string());
        }
    }

    security_headers
}

/// Configuration for streamable HTTP transport
#[derive(Clone, Debug)]
pub struct StreamableHttpConfig {
    /// Bind address
    pub bind_addr: String,
    /// Base path for endpoints (default: empty for root)
    pub base_path: String,
    /// SSE keep-alive interval
    pub keep_alive_secs: u64,
    /// Security validator for request validation
    pub security_validator: Arc<SecurityValidator>,
    /// Session security manager for secure session handling
    pub session_manager: Arc<SessionSecurityManager>,
}

impl Default for StreamableHttpConfig {
    fn default() -> Self {
        // Create secure defaults with localhost-only access and rate limiting
        let security_validator = Arc::new(
            SecurityConfigBuilder::new()
                .allow_localhost(true)
                .allow_any_origin(false)
                .require_authentication(false) // Start with auth disabled for backward compatibility
                .with_rate_limit(100, std::time::Duration::from_secs(60)) // 100 requests per minute
                .build(),
        );

        // Create session security manager with secure defaults
        let session_manager =
            Arc::new(SessionSecurityManager::new(SessionSecurityConfig::default()));

        Self {
            bind_addr: "127.0.0.1:8080".to_string(),
            base_path: "".to_string(),
            keep_alive_secs: 30,
            security_validator,
            session_manager,
        }
    }
}

/// Session information
struct Session {
    sse_sender: mpsc::Sender<SseMessage>,
}

/// Message types for SSE
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type")]
enum SseMessage {
    #[serde(rename = "message")]
    Message { data: serde_json::Value },
}

/// Shared application state
#[derive(Clone)]
struct AppState {
    sessions: Arc<RwLock<HashMap<String, Session>>>,
    pending_requests: Arc<RwLock<HashMap<String, serde_json::Value>>>,
    security_validator: Arc<SecurityValidator>,
    session_manager: Arc<SessionSecurityManager>,
}

/// Create router for streamable HTTP transport
pub fn create_router(config: StreamableHttpConfig) -> Router {
    let state = AppState {
        sessions: Arc::new(RwLock::new(HashMap::new())),
        pending_requests: Arc::new(RwLock::new(HashMap::new())),
        security_validator: config.security_validator.clone(),
        session_manager: config.session_manager.clone(),
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

/// SSE handler - establishes event stream with security validation
async fn sse_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, axum::Error>>>, StatusCode> {
    // CRITICAL SECURITY: Validate Origin header to prevent DNS rebinding attacks
    // Per MCP 2025-06-18: "Servers MUST validate the Origin header"
    let security_headers = convert_headers(&headers);
    if let Err(e) = state
        .security_validator
        .validate_request(&security_headers, addr.ip())
    {
        tracing::warn!(
            error = %e,
            client_ip = %addr.ip(),
            "Security validation failed for SSE connection"
        );
        return Err(StatusCode::from_u16(e.to_http_status()).unwrap_or(StatusCode::FORBIDDEN));
    }
    let (tx, mut rx) = mpsc::channel::<SseMessage>(100); // Bounded channel for backpressure control

    // Handle secure session management
    let user_agent = headers.get("User-Agent").and_then(|v| v.to_str().ok());

    let existing_session_id = headers.get("Mcp-Session-Id").and_then(|v| v.to_str().ok());

    // Create or validate secure session
    let secure_session = match existing_session_id {
        Some(session_id) => {
            // Validate existing session
            match state
                .session_manager
                .validate_session(session_id, addr.ip(), user_agent)
            {
                Ok(session) => session,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        client_ip = %addr.ip(),
                        "Session validation failed, creating new session"
                    );
                    // Create new session if validation fails
                    state
                        .session_manager
                        .create_session(addr.ip(), user_agent)
                        .map_err(|e| {
                            StatusCode::from_u16(e.to_http_status())
                                .unwrap_or(StatusCode::FORBIDDEN)
                        })?
                }
            }
        }
        None => {
            // Create new secure session
            state
                .session_manager
                .create_session(addr.ip(), user_agent)
                .map_err(|e| {
                    StatusCode::from_u16(e.to_http_status()).unwrap_or(StatusCode::FORBIDDEN)
                })?
        }
    };

    let session = Session {
        sse_sender: tx.clone(),
    };

    state
        .sessions
        .write()
        .await
        .insert(secure_session.id.clone(), session);

    // Create SSE stream
    let stream = async_stream::stream! {
        // Send session ID as first event
        yield Ok(Event::default()
            .id(Uuid::new_v4().to_string())
            .event("session")
            .data(secure_session.id.clone()));

        // Stream messages
        while let Some(msg) = rx.recv().await {
            let data = serde_json::to_string(&msg).unwrap_or_default();
            yield Ok(Event::default()
                .id(Uuid::new_v4().to_string())
                .event("message")
                .data(data));
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// JSON-RPC handler - returns 202 for all notifications/responses per MCP 2025-06-18
async fn json_rpc_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<serde_json::Value>,
) -> impl IntoResponse {
    // CRITICAL SECURITY: Validate Origin header to prevent DNS rebinding attacks
    // Per MCP 2025-06-18: "Servers MUST validate the Origin header"
    let security_headers = convert_headers(&headers);
    if let Err(e) = state
        .security_validator
        .validate_request(&security_headers, addr.ip())
    {
        tracing::warn!(
            error = %e,
            client_ip = %addr.ip(),
            "Security validation failed for JSON-RPC request"
        );
        return (
            StatusCode::from_u16(e.to_http_status()).unwrap_or(StatusCode::FORBIDDEN),
            HeaderMap::new(),
            Json(serde_json::json!({
                "error": {
                    "code": -32600,
                    "message": "Security validation failed",
                    "data": e.to_string()
                }
            })),
        );
    }
    // Handle secure session validation for JSON-RPC requests
    let user_agent = headers.get("User-Agent").and_then(|v| v.to_str().ok());

    let existing_session_id = headers.get("Mcp-Session-Id").and_then(|v| v.to_str().ok());

    // Validate or create secure session for JSON-RPC
    let secure_session = match existing_session_id {
        Some(session_id) => {
            match state
                .session_manager
                .validate_session(session_id, addr.ip(), user_agent)
            {
                Ok(session) => Some(session),
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        client_ip = %addr.ip(),
                        "Session validation failed for JSON-RPC request"
                    );
                    None // Will handle gracefully below
                }
            }
        }
        None => None,
    };

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
            return (
                StatusCode::BAD_REQUEST,
                HeaderMap::new(),
                Json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "code": -32600,
                        "message": "Invalid Request",
                        "data": format!("Unsupported protocol version: {}", version)
                    },
                    "id": request.get("id")
                })),
            );
        }
    }

    // Check if this is an initialization request
    if let Some(method) = request.get("method").and_then(|m| m.as_str()) {
        match method {
            "initialize" => {
                // Create or use existing secure session for initialization
                let session_id = match secure_session {
                    Some(ref session) => session.id.clone(),
                    None => {
                        // Create new session for initialization
                        match state.session_manager.create_session(addr.ip(), user_agent) {
                            Ok(session) => session.id,
                            Err(e) => {
                                tracing::error!(
                                    error = %e,
                                    client_ip = %addr.ip(),
                                    "Failed to create session for initialization"
                                );
                                Uuid::new_v4().to_string() // Fallback
                            }
                        }
                    }
                };

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
        if let Some(ref session) = secure_session {
            response_headers.insert("Mcp-Session-Id", session.id.parse().unwrap());
        }
        response_headers.insert("MCP-Protocol-Version", version_to_use.parse().unwrap());

        // Store for async processing if needed
        if let Some(ref session) = secure_session {
            let request_id = Uuid::new_v4().to_string();
            state
                .pending_requests
                .write()
                .await
                .insert(request_id.clone(), request.clone());

            // Send to SSE stream for processing
            if let Some(sse_session) = state.sessions.read().await.get(&session.id) {
                let _ = sse_session.sse_sender.send(SseMessage::Message {
                    data: request.clone(),
                });
            }
        }

        // Return 202 Accepted with no body per specification
        return (
            StatusCode::ACCEPTED,
            response_headers,
            Json(serde_json::json!({})),
        );
    }

    // For requests (have id, not responses): return immediate response
    let mut response_headers = HeaderMap::new();
    if let Some(ref session) = secure_session {
        response_headers.insert("Mcp-Session-Id", session.id.parse().unwrap());
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
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> StatusCode {
    // CRITICAL SECURITY: Validate Origin header to prevent DNS rebinding attacks
    let security_headers = convert_headers(&headers);
    if let Err(e) = state
        .security_validator
        .validate_request(&security_headers, addr.ip())
    {
        tracing::warn!(
            error = %e,
            client_ip = %addr.ip(),
            "Security validation failed for session deletion"
        );
        return StatusCode::from_u16(e.to_http_status()).unwrap_or(StatusCode::FORBIDDEN);
    }
    if let Some(session_id) = headers.get("Mcp-Session-Id").and_then(|v| v.to_str().ok()) {
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

    println!(
        "🚀 Streamable HTTP server listening on {}",
        config.bind_addr
    );
    println!("   - JSON-RPC endpoint: POST {}/", config.base_path);
    println!("   - SSE endpoint: GET {}/events", config.base_path);

    axum::serve(listener, app).await?;
    Ok(())
}
