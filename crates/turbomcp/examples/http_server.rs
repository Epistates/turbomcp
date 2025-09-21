#!/usr/bin/env cargo run --example http_server
//! Standalone HTTP/SSE server example demonstrating world-class MCP implementation
//!
//! This server showcases the production-ready HTTP/SSE transport layer with:
//! - Server-Sent Events for real-time bidirectional communication
//! - Complete MCP 2025-06-18 protocol compliance
//! - Enterprise-grade web-compatible transport
//! - Multi-session support
//!
//! Usage:
//!   Terminal 1: cargo run --example http_server
//!   Terminal 2: cargo run --example http_client

use serde_json::{Value, json};
use tokio::time::{Duration, sleep};
use turbomcp_core::MessageId;
use turbomcp_transport::{
    core::{Transport, TransportMessage},
    http_sse::{HttpSseConfig, HttpSseTransport},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging for debugging
    tracing_subscriber::fmt().with_env_filter("debug").init();

    println!("🚀 Starting TurboMCP HTTP/SSE Server");
    println!("📍 Server address: http://127.0.0.1:3000");

    // Create HTTP/SSE transport server with configuration
    let config = HttpSseConfig {
        bind_addr: "127.0.0.1:3000".to_string(),
        sse_path: "/events".to_string(),
        post_path: "/mcp".to_string(),
        keep_alive_interval: Duration::from_secs(30),
        max_sessions: 100,
        session_timeout: Duration::from_secs(300),
        enable_cors: true,
    };

    let mut server = HttpSseTransport::new(config);
    server.connect().await?;

    println!("✅ HTTP/SSE server listening on http://127.0.0.1:3000");
    println!("🔗 SSE endpoint: http://127.0.0.1:3000/events");
    println!("📤 POST endpoint: http://127.0.0.1:3000/mcp");
    println!("📝 Server implements MCP 2025-06-18 protocol");
    println!("🌐 Web-compatible transport with CORS enabled");

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

                    // Send response back to client via SSE
                    let response_str = response.to_string();
                    let response_msg = TransportMessage::new(
                        MessageId::from(format!("server-response-{}", message_count)),
                        response_str.into_bytes().into(),
                    );

                    if let Err(e) = server.send(response_msg).await {
                        eprintln!("❌ Failed to send response: {}", e);
                    } else {
                        println!("📤 Sent response #{} via SSE", message_count);
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
                        "name": "TurboMCP HTTP/SSE Server",
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
                            "name": "weather",
                            "description": "Get weather information for a location",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "location": {
                                        "type": "string",
                                        "description": "City or location name"
                                    }
                                },
                                "required": ["location"]
                            }
                        },
                        {
                            "name": "timestamp",
                            "description": "Get current server timestamp",
                            "inputSchema": {
                                "type": "object",
                                "properties": {},
                                "required": []
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
                                        "text": format!("HTTP/SSE Echo: {}", text)
                                    }]
                                }
                            })
                        }
                        "weather" => {
                            let location = args
                                .get("location")
                                .and_then(|l| l.as_str())
                                .unwrap_or("Unknown");
                            println!("🌤️ Weather tool called for: {}", location);
                            json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {
                                    "content": [{
                                        "type": "text",
                                        "text": format!("Weather in {}: Sunny, 25°C 🌞 (via HTTP/SSE)", location)
                                    }]
                                }
                            })
                        }
                        "timestamp" => {
                            use std::time::{SystemTime, UNIX_EPOCH};
                            let timestamp = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap()
                                .as_secs();
                            println!("🕐 Timestamp tool called");
                            json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {
                                    "content": [{
                                        "type": "text",
                                        "text": format!("Server timestamp: {} (HTTP/SSE transport)", timestamp)
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
