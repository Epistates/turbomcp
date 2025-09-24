//! WebSocket Transport Server - Real-time Bidirectional MCP
//!
//! This example demonstrates the WebSocket transport which enables
//! real-time bidirectional communication for live updates.
//!
//! Run with: `cargo run --example transport_websocket_server`

use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, RwLock};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use turbomcp::prelude::*;

/// Chat service using WebSocket transport (macro approach)
#[derive(Clone)]
struct ChatService {
    messages: Arc<RwLock<Vec<ChatMessage>>>,
    users: Arc<RwLock<Vec<String>>>,
    // WebSocket connections management
    connections: Arc<Mutex<HashMap<String, tokio::sync::mpsc::UnboundedSender<Message>>>>,
}

#[derive(Clone, Debug)]
struct ChatMessage {
    user: String,
    message: String,
    timestamp: chrono::DateTime<chrono::Utc>,
}

#[server(
    name = "Real-time Chat Service",
    version = "1.0.0",
    description = "Live chat with WebSocket transport"
)]
impl ChatService {
    fn new() -> Self {
        Self {
            messages: Arc::new(RwLock::new(Vec::new())),
            users: Arc::new(RwLock::new(Vec::new())),
            connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[tool("Send a chat message")]
    async fn send_message(&self, user: String, message: String) -> McpResult<String> {
        let chat_message = ChatMessage {
            user: user.clone(),
            message: message.clone(),
            timestamp: chrono::Utc::now(),
        };

        let mut messages = self.messages.write().await;
        messages.push(chat_message);

        // Keep only last 100 messages
        if messages.len() > 100 {
            let drain_count = messages.len() - 100;
            messages.drain(0..drain_count);
        }

        Ok(format!("✅ Message sent by {}: {}", user, message))
    }

    #[tool("Join the chat")]
    async fn join_chat(&self, user: String) -> McpResult<String> {
        let mut users = self.users.write().await;
        if !users.contains(&user) {
            users.push(user.clone());
            Ok(format!(
                "👋 {} joined the chat! Users online: {}",
                user,
                users.len()
            ))
        } else {
            Ok(format!("ℹ️  {} is already in the chat", user))
        }
    }

    #[tool("Leave the chat")]
    async fn leave_chat(&self, user: String) -> McpResult<String> {
        let mut users = self.users.write().await;
        if let Some(pos) = users.iter().position(|u| u == &user) {
            users.remove(pos);
            Ok(format!(
                "👋 {} left the chat. Users online: {}",
                user,
                users.len()
            ))
        } else {
            Ok(format!("ℹ️  {} was not in the chat", user))
        }
    }

    #[tool("Get recent messages")]
    async fn get_messages(&self, limit: Option<u32>) -> McpResult<String> {
        let messages = self.messages.read().await;
        let limit = limit.unwrap_or(10).min(50) as usize;

        let recent_messages: Vec<String> = messages
            .iter()
            .rev()
            .take(limit)
            .rev()
            .map(|msg| {
                format!(
                    "[{}] {}: {}",
                    msg.timestamp.format("%H:%M:%S"),
                    msg.user,
                    msg.message
                )
            })
            .collect();

        if recent_messages.is_empty() {
            Ok("💬 No messages yet. Be the first to send one!".to_string())
        } else {
            Ok(format!(
                "💬 Recent messages:\n{}",
                recent_messages.join("\n")
            ))
        }
    }

    #[tool("Get online users")]
    async fn get_users(&self) -> McpResult<String> {
        let users = self.users.read().await;
        if users.is_empty() {
            Ok("👥 No users online".to_string())
        } else {
            Ok(format!(
                "👥 Online users ({}): {}",
                users.len(),
                users.join(", ")
            ))
        }
    }

    #[resource("ws://chat/stats")]
    async fn get_stats(&self, _ctx: Context) -> McpResult<String> {
        let messages = self.messages.read().await;
        let users = self.users.read().await;

        Ok(format!(
            "📊 Chat Statistics:\n\
             💬 Total messages: {}\n\
             👥 Users online: {}\n\
             🕐 Server uptime: Active\n\
             🔄 Transport: WebSocket (Real-time)",
            messages.len(),
            users.len()
        ))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // CRITICAL: For MCP STDIO protocol, logs MUST go to stderr, not stdout
    // stdout is reserved for pure JSON-RPC messages only
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr) // Fix: Send logs to stderr
        .init();

    tracing::info!("💬 Starting Chat Service (WebSocket Transport)");
    tracing::info!("WebSocket server will be available at: ws://localhost:8081");
    tracing::info!("Features: Real-time messaging, live updates, bidirectional communication");

    let service = ChatService::new();

    // Start real WebSocket server using world-class tokio-tungstenite
    run_websocket_server(service, "127.0.0.1:8081").await?;

    Ok(())
}

/// Real WebSocket server implementation using world-class tokio-tungstenite
async fn run_websocket_server(
    service: ChatService,
    addr: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("🚀 WebSocket server listening on {}", addr);

    while let Ok((stream, peer_addr)) = listener.accept().await {
        let service_clone = service.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_websocket_connection(stream, service_clone, peer_addr).await {
                tracing::error!("WebSocket connection error for {}: {}", peer_addr, e);
            }
        });
    }

    Ok(())
}

/// Handle individual WebSocket connection with full MCP JSON-RPC support
async fn handle_websocket_connection(
    stream: TcpStream,
    service: ChatService,
    peer_addr: std::net::SocketAddr,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("🔗 New WebSocket connection from {}", peer_addr);

    // Upgrade TCP stream to WebSocket using world-class tokio-tungstenite
    let ws_stream = accept_async(stream).await?;
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    // Create channel for sending responses back to this WebSocket client
    let (response_tx, mut response_rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
    let connection_id = format!("ws-{}", peer_addr);

    // Register this connection
    {
        let mut connections = service.connections.lock().await;
        connections.insert(connection_id.clone(), response_tx.clone());
    }

    // Spawn task to handle outgoing messages to this WebSocket client
    let send_task = tokio::spawn(async move {
        while let Some(message) = response_rx.recv().await {
            if let Err(e) = ws_sender.send(message).await {
                tracing::error!("Failed to send WebSocket message: {}", e);
                break;
            }
        }
        tracing::debug!("WebSocket send task finished for {}", peer_addr);
    });

    // Handle incoming WebSocket messages with full MCP JSON-RPC processing
    while let Some(message) = ws_receiver.next().await {
        match message? {
            Message::Text(text) => {
                tracing::debug!("Received WebSocket message: {}", text);

                // Parse JSON-RPC message
                match serde_json::from_str::<Value>(&text) {
                    Ok(json_rpc) => {
                        // Process the JSON-RPC request using the service
                        if let Some(response) = process_mcp_request(&service, json_rpc).await {
                            let response_text = serde_json::to_string(&response)?;
                            if let Err(e) = response_tx.send(Message::Text(response_text.into())) {
                                tracing::error!("Failed to queue response: {}", e);
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Invalid JSON-RPC from {}: {}", peer_addr, e);
                        // Send JSON-RPC error response
                        let error_response = json!({
                            "jsonrpc": "2.0",
                            "id": null,
                            "error": {
                                "code": -32700,
                                "message": "Parse error"
                            }
                        });
                        let _ = response_tx.send(Message::Text(
                            serde_json::to_string(&error_response)?.into(),
                        ));
                    }
                }
            }
            Message::Close(_) => {
                tracing::info!("WebSocket connection closed by {}", peer_addr);
                break;
            }
            Message::Ping(data) => {
                // Respond to ping with pong
                let _ = response_tx.send(Message::Pong(data));
            }
            _ => {
                // Ignore other message types
            }
        }
    }

    // Clean up connection
    {
        let mut connections = service.connections.lock().await;
        connections.remove(&connection_id);
    }
    send_task.abort();

    tracing::info!("WebSocket connection from {} closed", peer_addr);
    Ok(())
}

/// Process MCP JSON-RPC requests and return responses
async fn process_mcp_request(service: &ChatService, request: Value) -> Option<Value> {
    let method = request.get("method")?.as_str()?;
    let id = request.get("id").cloned();
    let params = request.get("params");

    let result = match method {
        "initialize" => Some(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "Real-time Chat Service",
                "version": "1.0.0"
            }
        })),
        "tools/list" => Some(json!({
            "tools": [
                {
                    "name": "send_message",
                    "description": "Send a chat message",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "user": {"type": "string"},
                            "message": {"type": "string"}
                        },
                        "required": ["user", "message"]
                    }
                },
                {
                    "name": "join_chat",
                    "description": "Join the chat",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "user": {"type": "string"}
                        },
                        "required": ["user"]
                    }
                },
                {
                    "name": "get_messages",
                    "description": "Get recent messages",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "limit": {"type": "number"}
                        }
                    }
                }
            ]
        })),
        "tools/call" => {
            if let Some(params) = params {
                let tool_name = params.get("name")?.as_str()?;
                match tool_name {
                    "send_message" => {
                        let args = params.get("arguments")?;
                        let user = args.get("user")?.as_str()?.to_string();
                        let message = args.get("message")?.as_str()?.to_string();

                        let chat_message = ChatMessage {
                            user: user.clone(),
                            message: message.clone(),
                            timestamp: chrono::Utc::now(),
                        };

                        // Add message to chat
                        let mut messages = service.messages.write().await;
                        messages.push(chat_message);
                        if messages.len() > 100 {
                            let drain_count = messages.len() - 100;
                            messages.drain(0..drain_count);
                        }

                        Some(json!({
                            "content": [{
                                "type": "text",
                                "text": format!("✅ Message sent by {}: {}", user, message)
                            }]
                        }))
                    }
                    "join_chat" => {
                        let args = params.get("arguments")?;
                        let user = args.get("user")?.as_str()?.to_string();

                        let mut users = service.users.write().await;
                        if !users.contains(&user) {
                            users.push(user.clone());
                        }

                        Some(json!({
                            "content": [{
                                "type": "text",
                                "text": format!("👋 {} joined the chat! Users online: {}", user, users.len())
                            }]
                        }))
                    }
                    "get_messages" => {
                        let limit = params
                            .get("arguments")
                            .and_then(|a| a.get("limit"))
                            .and_then(|l| l.as_u64())
                            .unwrap_or(10)
                            .min(50) as usize;

                        let messages = service.messages.read().await;
                        let recent: Vec<String> = messages
                            .iter()
                            .rev()
                            .take(limit)
                            .rev()
                            .map(|msg| {
                                format!(
                                    "[{}] {}: {}",
                                    msg.timestamp.format("%H:%M:%S"),
                                    msg.user,
                                    msg.message
                                )
                            })
                            .collect();

                        let text = if recent.is_empty() {
                            "💬 No messages yet. Be the first to send one!".to_string()
                        } else {
                            format!("💬 Recent messages:\n{}", recent.join("\n"))
                        };

                        Some(json!({
                            "content": [{
                                "type": "text",
                                "text": text
                            }]
                        }))
                    }
                    _ => Some(json!({
                        "error": {
                            "code": -32601,
                            "message": format!("Unknown tool: {}", tool_name)
                        }
                    })),
                }
            } else {
                None
            }
        }
        _ => Some(json!({
            "error": {
                "code": -32601,
                "message": format!("Method not found: {}", method)
            }
        })),
    };

    result.map(|result| {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        })
    })
}
