//! WebSocket transport implementation for MCP servers

use crate::cli::Connection;
use futures::{SinkExt, StreamExt};
use serde_json::json;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

/// List tools from WebSocket MCP server
pub async fn list_tools(conn: &Connection) -> Result<serde_json::Value, String> {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list",
        "params": {}
    });

    send_request(conn, request).await
}

/// Call a tool on WebSocket MCP server
pub async fn call_tool(
    conn: &Connection,
    name: String,
    arguments: String,
) -> Result<serde_json::Value, String> {
    let args: serde_json::Value =
        serde_json::from_str(&arguments).map_err(|e| format!("Invalid JSON arguments: {e}"))?;

    let request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": name,
            "arguments": args
        }
    });

    send_request(conn, request).await
}

/// Get tool schemas from WebSocket MCP server
pub async fn get_schemas(conn: &Connection) -> Result<serde_json::Value, String> {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/list",
        "params": {}
    });

    let response = send_request(conn, request).await?;
    super::common::extract_schemas(response)
}

/// Send a JSON-RPC request over WebSocket and receive response
async fn send_request(
    conn: &Connection,
    request: serde_json::Value,
) -> Result<serde_json::Value, String> {
    // Convert HTTP/HTTPS URL to WebSocket URL
    let ws_url = conn
        .url
        .replace("http://", "ws://")
        .replace("https://", "wss://")
        .replace("/mcp", "/ws");

    // Connect to WebSocket server
    let (ws_stream, _) = connect_async(&ws_url)
        .await
        .map_err(|e| format!("Failed to connect to WebSocket at {ws_url}: {e}"))?;

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    // Send the JSON-RPC request
    let request_text =
        serde_json::to_string(&request).map_err(|e| format!("Failed to serialize request: {e}"))?;

    ws_sender
        .send(Message::Text(request_text.into()))
        .await
        .map_err(|e| format!("Failed to send WebSocket message: {e}"))?;

    // Wait for response
    match ws_receiver.next().await {
        Some(Ok(Message::Text(response_text))) => serde_json::from_str(&response_text)
            .map_err(|e| format!("Failed to parse JSON response: {e}")),
        Some(Ok(msg)) => Err(format!("Unexpected WebSocket message type: {msg:?}")),
        Some(Err(e)) => Err(format!("WebSocket error: {e}")),
        None => Err("WebSocket connection closed unexpectedly".to_string()),
    }
}
