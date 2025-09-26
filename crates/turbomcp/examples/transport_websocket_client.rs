//! WebSocket Transport Client - Real-time Bidirectional MCP
//!
//! This example demonstrates connecting to a WebSocket-based MCP server
//! for real-time bidirectional communication and live updates.
//!
//! Run with: `cargo run --example transport_websocket_client`

use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};

use tokio_tungstenite::{connect_async, tungstenite::Message};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // CRITICAL: For MCP STDIO protocol, logs MUST go to stderr, not stdout
    // stdout is reserved for pure JSON-RPC messages only
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr) // Fix: Send logs to stderr
        .init();

    tracing::info!("💬 Starting WebSocket Transport Client");
    tracing::info!("🔗 Connecting to WebSocket server at ws://localhost:8080");

    // Connect to WebSocket server using tokio-tungstenite
    let (ws_stream, _response) = connect_async("ws://localhost:8080").await?;
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    tracing::info!("✅ Connected to WebSocket server");

    // Initialize the MCP connection
    let init_message = json!({
        "jsonrpc": "2.0",
        "id": "init-1",
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "WebSocket Chat Client",
                "version": "1.0.0"
            }
        }
    });

    ws_sender
        .send(Message::Text(init_message.to_string().into()))
        .await?;
    tracing::info!("📋 Sent initialize request");

    // Wait for initialize response
    if let Some(msg) = ws_receiver.next().await
        && let Message::Text(text) = msg?
    {
        let response: Value = serde_json::from_str(&text)?;
        if let Some(server_info) = response.get("result").and_then(|r| r.get("serverInfo")) {
            tracing::info!(
                "🎯 Server: {}",
                server_info.get("name").unwrap_or(&json!("Unknown"))
            );
            tracing::info!(
                "🔧 Version: {}",
                server_info.get("version").unwrap_or(&json!("Unknown"))
            );
        }
    }

    // List available tools
    let tools_message = json!({
        "jsonrpc": "2.0",
        "id": "tools-1",
        "method": "tools/list",
        "params": {}
    });

    ws_sender
        .send(Message::Text(tools_message.to_string().into()))
        .await?;
    tracing::info!("🛠️  Requested tools list");

    // Wait for tools response
    if let Some(msg) = ws_receiver.next().await
        && let Message::Text(text) = msg?
    {
        let response: Value = serde_json::from_str(&text)?;
        if let Some(tools) = response
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(|t| t.as_array())
        {
            tracing::info!("🛠️  Available tools: {}", tools.len());
            for tool in tools {
                if let Some(name) = tool.get("name") {
                    tracing::info!("  - {}", name);
                }
            }
        }
    }

    // Test chat operations
    tracing::info!("💬 Testing real-time chat operations...");

    // Join the chat
    let join_message = json!({
        "jsonrpc": "2.0",
        "id": "join-1",
        "method": "tools/call",
        "params": {
            "name": "join_chat",
            "arguments": {
                "user": "WebSocketClient"
            }
        }
    });

    ws_sender
        .send(Message::Text(join_message.to_string().into()))
        .await?;
    tracing::info!("👋 Sent join chat request");

    // Wait for join response
    if let Some(msg) = ws_receiver.next().await
        && let Message::Text(text) = msg?
    {
        let response: Value = serde_json::from_str(&text)?;
        if let Some(content) = response
            .get("result")
            .and_then(|r| r.get("content"))
            .and_then(|c| c.as_array())
            && let Some(text_content) = content.first().and_then(|item| item.get("text"))
        {
            tracing::info!("✅ {}", text_content);
        }
    }

    // Send a message
    let send_message = json!({
        "jsonrpc": "2.0",
        "id": "send-1",
        "method": "tools/call",
        "params": {
            "name": "send_message",
            "arguments": {
                "user": "WebSocketClient",
                "message": "Hello from WebSocket client! Real-time communication is working perfectly!"
            }
        }
    });

    ws_sender
        .send(Message::Text(send_message.to_string().into()))
        .await?;
    tracing::info!("💬 Sent chat message");

    // Wait for send response
    if let Some(msg) = ws_receiver.next().await
        && let Message::Text(text) = msg?
    {
        let response: Value = serde_json::from_str(&text)?;
        if let Some(content) = response
            .get("result")
            .and_then(|r| r.get("content"))
            .and_then(|c| c.as_array())
            && let Some(text_content) = content.first().and_then(|item| item.get("text"))
        {
            tracing::info!("✅ {}", text_content);
        }
    }

    // Send another message
    let send_message2 = json!({
        "jsonrpc": "2.0",
        "id": "send-2",
        "method": "tools/call",
        "params": {
            "name": "send_message",
            "arguments": {
                "user": "WebSocketClient",
                "message": "WebSocket transport provides excellent real-time capabilities!"
            }
        }
    });

    ws_sender
        .send(Message::Text(send_message2.to_string().into()))
        .await?;
    tracing::info!("💬 Sent second chat message");

    // Wait for second send response
    if let Some(msg) = ws_receiver.next().await
        && let Message::Text(text) = msg?
    {
        let response: Value = serde_json::from_str(&text)?;
        if let Some(content) = response
            .get("result")
            .and_then(|r| r.get("content"))
            .and_then(|c| c.as_array())
            && let Some(text_content) = content.first().and_then(|item| item.get("text"))
        {
            tracing::info!("✅ {}", text_content);
        }
    }

    // Get recent messages
    let get_messages = json!({
        "jsonrpc": "2.0",
        "id": "get-1",
        "method": "tools/call",
        "params": {
            "name": "get_messages",
            "arguments": {
                "limit": 5
            }
        }
    });

    ws_sender
        .send(Message::Text(get_messages.to_string().into()))
        .await?;
    tracing::info!("📖 Requested recent messages");

    // Wait for messages response
    if let Some(msg) = ws_receiver.next().await
        && let Message::Text(text) = msg?
    {
        let response: Value = serde_json::from_str(&text)?;
        if let Some(content) = response
            .get("result")
            .and_then(|r| r.get("content"))
            .and_then(|c| c.as_array())
            && let Some(text_content) = content.first().and_then(|item| item.get("text"))
        {
            tracing::info!("📖 {}", text_content);
        }
    }

    // Get online users
    let get_users = json!({
        "jsonrpc": "2.0",
        "id": "users-1",
        "method": "tools/call",
        "params": {
            "name": "get_users",
            "arguments": {}
        }
    });

    ws_sender
        .send(Message::Text(get_users.to_string().into()))
        .await?;
    tracing::info!("👥 Requested online users");

    // Wait for users response
    if let Some(msg) = ws_receiver.next().await
        && let Message::Text(text) = msg?
    {
        let response: Value = serde_json::from_str(&text)?;
        if let Some(content) = response
            .get("result")
            .and_then(|r| r.get("content"))
            .and_then(|c| c.as_array())
            && let Some(text_content) = content.first().and_then(|item| item.get("text"))
        {
            tracing::info!("👥 {}", text_content);
        }
    }

    // Leave the chat
    let leave_message = json!({
        "jsonrpc": "2.0",
        "id": "leave-1",
        "method": "tools/call",
        "params": {
            "name": "leave_chat",
            "arguments": {
                "user": "WebSocketClient"
            }
        }
    });

    ws_sender
        .send(Message::Text(leave_message.to_string().into()))
        .await?;
    tracing::info!("👋 Sent leave chat request");

    // Wait for leave response
    if let Some(msg) = ws_receiver.next().await
        && let Message::Text(text) = msg?
    {
        let response: Value = serde_json::from_str(&text)?;
        if let Some(content) = response
            .get("result")
            .and_then(|r| r.get("content"))
            .and_then(|c| c.as_array())
            && let Some(text_content) = content.first().and_then(|item| item.get("text"))
        {
            tracing::info!("✅ {}", text_content);
        }
    }

    // Close the connection gracefully
    ws_sender.send(Message::Close(None)).await?;
    tracing::info!("🔚 WebSocket client demo completed");
    tracing::info!(
        "💡 WebSocket transport provides excellent real-time bidirectional communication!"
    );

    Ok(())
}
