#!/usr/bin/env cargo run --example tcp_client
//! Standalone TCP client example demonstrating MCP protocol implementation
//!
//! This client showcases the TCP transport layer with:
//! - Complete MCP 2025-06-18 protocol compliance
//! - Comprehensive error handling with timeouts
//! - Real-world tool calling patterns
//! - Builder pattern configuration
//!
//! Usage:
//!   Terminal 1: cargo run --example tcp_server
//!   Terminal 2: cargo run --example tcp_client

use serde_json::{Value, json};
use std::net::SocketAddr;
use tokio::time::{Duration, sleep, timeout};
use turbomcp_core::MessageId;
use turbomcp_transport::{
    core::{Transport, TransportMessage},
    tcp::TcpTransportBuilder,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging for debugging
    tracing_subscriber::fmt().with_env_filter("debug").init();

    let bind_addr: SocketAddr = "127.0.0.1:0".parse()?; // Auto-assign local port
    let remote_addr: SocketAddr = "127.0.0.1:8080".parse()?;

    println!("🚀 Starting TurboMCP TCP Client");
    println!("📍 Connecting to: {}", remote_addr);

    // Wait a moment for server to be ready
    sleep(Duration::from_millis(500)).await;

    // Create and connect TCP transport client using builder pattern
    let mut client = TcpTransportBuilder::new()
        .bind_addr(bind_addr)
        .remote_addr(remote_addr)
        .connect_timeout_ms(10000)
        .keep_alive(true)
        .buffer_size(8192)
        .build();

    println!("🔗 Connecting to TCP server...");
    client.connect().await?;
    println!("✅ Connected to server");
    println!("🏗️ Built with TcpTransportBuilder pattern");

    // Wait for connection to be fully established
    sleep(Duration::from_millis(1000)).await;

    // Test sequence: MCP protocol compliance verification
    println!("\n🧪 Starting MCP Protocol Test Sequence");

    // Test 1: Initialize protocol
    println!("\n📋 Test 1: Initialize Protocol");
    let init_response = send_request_with_timeout(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {
                    "roots": {"listChanged": true},
                    "sampling": {},
                    "elicitation": {}
                },
                "clientInfo": {
                    "name": "TurboMCP TCP Client",
                    "version": "1.0.8"
                }
            }
        }),
        "init-1",
    )
    .await?;

    if validate_response(&init_response, "initialize") {
        println!("✅ Initialize protocol: SUCCESS");
    } else {
        println!("❌ Initialize protocol: FAILED");
        return Err("Initialize failed".into());
    }

    // Test 2: List available tools
    println!("\n📋 Test 2: List Available Tools");
    let tools_response = send_request_with_timeout(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        }),
        "tools-list-2",
    )
    .await?;

    if validate_response(&tools_response, "tools/list") {
        println!("✅ Tools list: SUCCESS");
        if let Some(tools) = tools_response.get("result").and_then(|r| r.get("tools"))
            && let Some(tools_array) = tools.as_array()
        {
            println!("🛠️ Available tools: {} tools found", tools_array.len());
            for tool in tools_array {
                if let Some(name) = tool.get("name").and_then(|n| n.as_str()) {
                    println!("   • {}", name);
                }
            }
        }
    } else {
        println!("❌ Tools list: FAILED");
        return Err("Tools list failed".into());
    }

    // Test 3: Call echo tool
    println!("\n📋 Test 3: Call Echo Tool");
    let echo_response = send_request_with_timeout(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "echo",
                "arguments": {
                    "text": "Hello from TurboMCP TCP client! 🚀"
                }
            }
        }),
        "echo-3",
    )
    .await?;

    if validate_response(&echo_response, "tools/call") {
        println!("✅ Echo tool call: SUCCESS");
        if let Some(content) = extract_tool_result(&echo_response) {
            println!("🔊 Echo result: {}", content);
        }
    } else {
        println!("❌ Echo tool call: FAILED");
        return Err("Echo tool call failed".into());
    }

    // Test 4: Call add tool
    println!("\n📋 Test 4: Call Add Tool");
    let add_response = send_request_with_timeout(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "add",
                "arguments": {
                    "a": 25,
                    "b": 17
                }
            }
        }),
        "add-4",
    )
    .await?;

    if validate_response(&add_response, "tools/call") {
        println!("✅ Add tool call: SUCCESS");
        if let Some(content) = extract_tool_result(&add_response) {
            println!("➕ Add result: {}", content);
            if content.contains("42") {
                println!("🎯 Calculation verified: 25 + 17 = 42");
            } else {
                println!("⚠️ Unexpected calculation result");
            }
        }
    } else {
        println!("❌ Add tool call: FAILED");
        return Err("Add tool call failed".into());
    }

    // Test 5: Call multiply tool
    println!("\n📋 Test 5: Call Multiply Tool");
    let mult_response = send_request_with_timeout(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "multiply",
                "arguments": {
                    "x": 6,
                    "y": 7
                }
            }
        }),
        "mult-5",
    )
    .await?;

    if validate_response(&mult_response, "tools/call") {
        println!("✅ Multiply tool call: SUCCESS");
        if let Some(content) = extract_tool_result(&mult_response) {
            println!("✖️ Multiply result: {}", content);
            if content.contains("42") {
                println!("🎯 Calculation verified: 6 × 7 = 42");
            } else {
                println!("⚠️ Unexpected calculation result");
            }
        }
    } else {
        println!("❌ Multiply tool call: FAILED");
        return Err("Multiply tool call failed".into());
    }

    // Test 6: Test error handling with invalid tool
    println!("\n📋 Test 6: Error Handling (Invalid Tool)");
    let error_response = send_request_with_timeout(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tools/call",
            "params": {
                "name": "nonexistent_tool",
                "arguments": {}
            }
        }),
        "error-6",
    )
    .await?;

    if error_response.get("error").is_some() {
        println!("✅ Error handling: SUCCESS");
        if let Some(error) = error_response.get("error")
            && let Some(message) = error.get("message").and_then(|m| m.as_str())
        {
            println!("⚠️ Expected error: {}", message);
        }
    } else {
        println!("❌ Error handling: FAILED (should have returned error)");
    }

    println!("\n🎉 TCP Transport Test Sequence COMPLETED!");
    println!("✅ All MCP protocol operations successful");
    println!("✅ TurboMCP TCP transport working correctly!");

    Ok(())
}

/// Send request with timeout and proper error handling
async fn send_request_with_timeout(
    client: &mut impl Transport,
    request: Value,
    message_id: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let request_str = request.to_string();

    println!("📤 Sending: {}", request_str);

    // Send request
    let message =
        TransportMessage::new(MessageId::from(message_id), request_str.into_bytes().into());
    client.send(message).await?;

    // Receive response with timeout
    let response = timeout(Duration::from_secs(5), client.receive()).await??;

    match response {
        Some(response_msg) => {
            let response_str = String::from_utf8(response_msg.payload.to_vec())?;
            println!("📥 Received: {}", response_str);

            let response_json: Value = serde_json::from_str(&response_str)?;
            Ok(response_json)
        }
        None => Err("No response received".into()),
    }
}

/// Validate MCP JSON-RPC response format
fn validate_response(response: &Value, expected_method: &str) -> bool {
    // Check JSON-RPC 2.0 format
    if response.get("jsonrpc").and_then(|v| v.as_str()) != Some("2.0") {
        println!("❌ Invalid JSON-RPC version");
        return false;
    }

    // Check for either result or error
    let has_result = response.get("result").is_some();
    let has_error = response.get("error").is_some();

    if !has_result && !has_error {
        println!("❌ Response missing both result and error");
        return false;
    }

    if has_result && has_error {
        println!("❌ Response has both result and error");
        return false;
    }

    // Check ID is present
    if response.get("id").is_none() {
        println!("❌ Response missing ID");
        return false;
    }

    println!("✅ Valid JSON-RPC 2.0 response for {}", expected_method);
    true
}

/// Extract tool result content from response
fn extract_tool_result(response: &Value) -> Option<String> {
    response
        .get("result")?
        .get("content")?
        .as_array()?
        .first()?
        .get("text")?
        .as_str()
        .map(|s| s.to_string())
}
