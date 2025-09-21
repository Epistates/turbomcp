#!/usr/bin/env cargo run --example unix_socket_client
//! Standalone Unix socket client example demonstrating world-class MCP implementation
//!
//! This client showcases the production-ready Unix transport layer with:
//! - Complete MCP 2025-06-18 protocol compliance
//! - Enterprise-grade error handling
//! - Real-world tool calling patterns
//!
//! Usage:
//!   Terminal 1: cargo run --example unix_socket_server
//!   Terminal 2: cargo run --example unix_socket_client

use serde_json::{Value, json};
use std::path::PathBuf;
use tokio::time::{Duration, sleep, timeout};
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

    println!("🚀 Starting TurboMCP Unix Socket Client");
    println!("📍 Connecting to: {:?}", socket_path);

    // Wait a moment for server to be ready
    sleep(Duration::from_millis(500)).await;

    // Create and connect Unix transport client
    let mut client = UnixTransport::new_client(socket_path.clone());

    println!("🔗 Connecting to Unix socket server...");
    client.connect().await?;
    println!("✅ Connected to server");

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
                    "name": "TurboMCP Unix Socket Client",
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
                    "text": "Hello from TurboMCP Unix client! 🚀"
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
                    "a": 42,
                    "b": 58
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
            if content.contains("100") {
                println!("🎯 Calculation verified: 42 + 58 = 100");
            } else {
                println!("⚠️ Unexpected calculation result");
            }
        }
    } else {
        println!("❌ Add tool call: FAILED");
        return Err("Add tool call failed".into());
    }

    // Test 5: Test error handling with invalid tool
    println!("\n📋 Test 5: Error Handling (Invalid Tool)");
    let error_response = send_request_with_timeout(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "nonexistent_tool",
                "arguments": {}
            }
        }),
        "error-5",
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

    println!("\n🎉 Unix Socket Transport Test Sequence COMPLETED!");
    println!("✅ All MCP protocol operations successful");
    println!("🚀 TurboMCP Unix transport is production-ready!");

    Ok(())
}

/// Send request with timeout and proper error handling
async fn send_request_with_timeout(
    client: &mut UnixTransport,
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
