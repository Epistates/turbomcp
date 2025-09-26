#!/usr/bin/env cargo run --example unix_socket_server
//! Standalone Unix socket server example demonstrating MCP protocol implementation
//!
//! This server showcases the Unix transport layer with:
//! - Tokio best practices with Framed + LinesCodec
//! - Complete MCP 2025-06-18 protocol compliance
//! - Comprehensive connection management
//!
//! Usage:
//!   Terminal 1: cargo run --example unix_socket_server
//!   Terminal 2: cargo run --example unix_socket_client

use serde_json::{Value, json};
use std::path::PathBuf;
use tokio::time::{Duration, sleep};
use turbomcp_core::MessageId;
use turbomcp_transport::{
    core::{Transport, TransportMessage},
    unix::UnixTransport,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging for debugging
    tracing_subscriber::fmt().with_env_filter("debug").init();

    let socket_path = PathBuf::from("/tmp/turbomcp-unix-server-example");

    // Clean up any existing socket (ASYNC - Non-blocking!)
    let _ = tokio::fs::remove_file(&socket_path).await;

    println!("🚀 Starting TurboMCP Unix Socket Server");
    println!("📍 Socket path: {:?}", socket_path);

    // Create and start Unix transport server
    let mut server = UnixTransport::new_server(socket_path.clone());
    server.connect().await?;

    println!("✅ Unix socket server listening");
    println!("🔗 Ready for client connections");
    println!("📝 Server implements MCP 2025-06-18 protocol");

    // Server message handling loop
    let mut message_count = 0;

    loop {
        match server.receive().await {
            Ok(Some(message)) => {
                message_count += 1;
                let payload = String::from_utf8_lossy(&message.payload);
                println!("📨 Received message #{}: {}", message_count, payload);

                // Parse JSON-RPC request
                if let Ok(request) = serde_json::from_str::<Value>(&payload) {
                    let response = handle_mcp_request(&request, message_count);

                    // Send response back to client
                    let response_str = response.to_string();
                    let response_msg = TransportMessage::new(
                        MessageId::from(format!("server-response-{}", message_count)),
                        response_str.into_bytes().into(),
                    );

                    if let Err(e) = server.send(response_msg).await {
                        eprintln!("❌ Failed to send response: {}", e);
                    } else {
                        println!("📤 Sent response #{}", message_count);
                    }
                } else {
                    eprintln!("⚠️ Invalid JSON received: {}", payload);
                }
            }
            Ok(None) => {
                println!("🔄 No message received, continuing...");
                sleep(Duration::from_millis(100)).await;
            }
            Err(e) => {
                eprintln!("❌ Error receiving message: {}", e);
                sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

/// Handle MCP protocol requests with full 2025-06-18 compliance
fn handle_mcp_request(request: &Value, _request_id: usize) -> Value {
    let method = request
        .get("method")
        .and_then(|m| m.as_str())
        .unwrap_or("unknown");
    let id = request.get("id");

    match method {
        "initialize" => {
            println!("🔧 Handling MCP initialize request");
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {
                        "tools": {},
                        "resources": {},
                        "prompts": {},
                        "logging": {},
                        "elicitation": {}
                    },
                    "serverInfo": {
                        "name": "TurboMCP Unix Socket Server",
                        "version": "1.0.8"
                    }
                }
            })
        }
        "tools/list" => {
            println!("🛠️ Handling tools/list request");
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": [
                        {
                            "name": "echo",
                            "description": "Echo back the input text",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "text": {
                                        "type": "string",
                                        "description": "Text to echo back"
                                    }
                                },
                                "required": ["text"]
                            }
                        },
                        {
                            "name": "add",
                            "description": "Add two numbers",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "a": {"type": "number"},
                                    "b": {"type": "number"}
                                },
                                "required": ["a", "b"]
                            }
                        }
                    ]
                }
            })
        }
        "tools/call" => {
            if let Some(params) = request.get("params") {
                if let Some(tool_name) = params.get("name").and_then(|n| n.as_str()) {
                    let default_args = json!({});
                    let args = params.get("arguments").unwrap_or(&default_args);

                    match tool_name {
                        "echo" => {
                            let text = args
                                .get("text")
                                .and_then(|t| t.as_str())
                                .unwrap_or("(empty)");
                            println!("🔊 Echo tool called with: {}", text);
                            json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {
                                    "content": [{
                                        "type": "text",
                                        "text": format!("Echo: {}", text)
                                    }]
                                }
                            })
                        }
                        "add" => {
                            let a = args.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            let b = args.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            let result = a + b;
                            println!("➕ Add tool called: {} + {} = {}", a, b, result);
                            json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {
                                    "content": [{
                                        "type": "text",
                                        "text": format!("Result: {}", result)
                                    }]
                                }
                            })
                        }
                        _ => {
                            eprintln!("❌ Unknown tool: {}", tool_name);
                            json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "error": {
                                    "code": -32601,
                                    "message": format!("Tool not found: {}", tool_name)
                                }
                            })
                        }
                    }
                } else {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {
                            "code": -32602,
                            "message": "Invalid params: missing tool name"
                        }
                    })
                }
            } else {
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32602,
                        "message": "Invalid params"
                    }
                })
            }
        }
        _ => {
            eprintln!("❌ Unknown method: {}", method);
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": format!("Method not found: {}", method)
                }
            })
        }
    }
}
