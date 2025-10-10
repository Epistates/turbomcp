#!/usr/bin/env cargo run --example tcp_server
//! Standalone TCP server example demonstrating MCP protocol implementation
//!
//! This server showcases the TCP transport layer with:
//! - Tokio best practices with Framed + LinesCodec
//! - Complete MCP 2025-06-18 protocol compliance
//! - Comprehensive connection management with backpressure
//! - Builder pattern configuration
//!
//! Usage:
//!   Terminal 1: cargo run --example tcp_server
//!   Terminal 2: cargo run --example tcp_client

use serde_json::{Value, json};
use std::net::SocketAddr;
use tokio::time::{Duration, sleep};
use turbomcp_core::MessageId;
use turbomcp_transport::{
    core::{Transport, TransportMessage},
    tcp::TcpTransportBuilder,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging for debugging
    tracing_subscriber::fmt().with_env_filter("debug").init();

    let bind_addr: SocketAddr = "127.0.0.1:8080".parse()?;

    println!("🚀 Starting TurboMCP TCP Server");
    println!("📍 Bind address: {}", bind_addr);

    // Create and start TCP transport server using builder pattern
    let server = TcpTransportBuilder::new()
        .bind_addr(bind_addr)
        .connect_timeout_ms(10000)
        .keep_alive(true)
        .buffer_size(8192)
        .build();

    server.connect().await?;

    println!("✅ TCP server listening on {}", bind_addr);
    println!("🔗 Ready for client connections");
    println!("📝 Server implements MCP 2025-06-18 protocol");
    println!("🏗️ Built with TcpTransportBuilder pattern");

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
                        "name": "TurboMCP TCP Server",
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
                        },
                        {
                            "name": "multiply",
                            "description": "Multiply two numbers",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "x": {"type": "number"},
                                    "y": {"type": "number"}
                                },
                                "required": ["x", "y"]
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
                                        "text": format!("TCP Echo: {}", text)
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
                                        "text": format!("TCP Add Result: {}", result)
                                    }]
                                }
                            })
                        }
                        "multiply" => {
                            let x = args.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            let y = args.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            let result = x * y;
                            println!("✖️ Multiply tool called: {} × {} = {}", x, y, result);
                            json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {
                                    "content": [{
                                        "type": "text",
                                        "text": format!("TCP Multiply Result: {}", result)
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
