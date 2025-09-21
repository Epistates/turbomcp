//! Standalone Unix transport verification test
//!
//! This test proves the Unix transport works correctly by running
//! a real server in a background task and connecting with a real client.

use serde_json::{Value, json};
use std::path::PathBuf;
use tokio::task::JoinHandle;
use tokio::time::{Duration, sleep, timeout};
use turbomcp_core::MessageId;
use turbomcp_transport::{
    core::{Transport, TransportMessage},
    unix::UnixTransport,
};

#[tokio::test]
async fn test_unix_transport_standalone_server_client() {
    println!("🚀 Unix Transport Standalone Verification Test");

    let socket_path = PathBuf::from("/tmp/turbomcp-standalone-test");
    let _ = std::fs::remove_file(&socket_path); // Clean up

    // Start server in background task
    let server_socket_path = socket_path.clone();
    let server_task: JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>> =
        tokio::spawn(async move { run_test_server(server_socket_path).await });

    // Give server time to start
    sleep(Duration::from_millis(1000)).await;

    // Run client test
    let client_result = run_test_client(socket_path.clone()).await;

    // Clean up
    server_task.abort();
    let _ = std::fs::remove_file(&socket_path);

    // Verify client test passed
    assert!(
        client_result.is_ok(),
        "Client test failed: {:?}",
        client_result
    );
    println!("🎉 Unix Transport Standalone Test PASSED!");
}

async fn run_test_server(
    socket_path: PathBuf,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("🔧 Starting test server on {:?}", socket_path);

    let mut server = UnixTransport::new_server(socket_path);
    server.connect().await?;
    println!("✅ Server listening");

    // Handle up to 5 requests for the test
    for request_num in 1..=5 {
        match timeout(Duration::from_secs(10), server.receive()).await {
            Ok(Ok(Some(message))) => {
                let payload = String::from_utf8_lossy(&message.payload);
                println!("📨 Server received request #{}: {}", request_num, payload);

                if let Ok(request) = serde_json::from_str::<Value>(&payload) {
                    let response = create_test_response(&request);
                    let response_str = response.to_string();
                    let response_msg = TransportMessage::new(
                        MessageId::from(format!("server-{}", request_num)),
                        response_str.into_bytes().into(),
                    );

                    if let Err(e) = server.send(response_msg).await {
                        eprintln!("❌ Server failed to send response: {}", e);
                        return Err(e.into());
                    } else {
                        println!("📤 Server sent response #{}", request_num);
                    }
                }
            }
            Ok(Ok(None)) => {
                println!("🔄 Server: no message");
                continue;
            }
            Ok(Err(e)) => {
                eprintln!("❌ Server receive error: {}", e);
                return Err(e.into());
            }
            Err(_) => {
                println!("⏰ Server timeout on request #{}", request_num);
                break;
            }
        }
    }

    Ok(())
}

async fn run_test_client(socket_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔗 Starting test client connecting to {:?}", socket_path);

    let mut client = UnixTransport::new_client(socket_path);
    client.connect().await?;
    println!("✅ Client connected");

    // Wait for connection to be established
    sleep(Duration::from_millis(500)).await;

    // Test 1: Initialize
    println!("\n📋 Test 1: Initialize");
    let response = send_and_receive(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "clientInfo": {"name": "test-client", "version": "1.0.0"}
            }
        }),
        "test-1",
    )
    .await?;

    assert_eq!(
        response.get("jsonrpc").and_then(|v| v.as_str()),
        Some("2.0")
    );
    assert!(response.get("result").is_some());
    println!("✅ Initialize test passed");

    // Test 2: Tools list
    println!("\n📋 Test 2: Tools List");
    let response = send_and_receive(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        }),
        "test-2",
    )
    .await?;

    assert_eq!(
        response.get("jsonrpc").and_then(|v| v.as_str()),
        Some("2.0")
    );
    assert!(response.get("result").is_some());
    println!("✅ Tools list test passed");

    // Test 3: Echo tool
    println!("\n📋 Test 3: Echo Tool");
    let response = send_and_receive(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "echo",
                "arguments": {"text": "test message"}
            }
        }),
        "test-3",
    )
    .await?;

    assert_eq!(
        response.get("jsonrpc").and_then(|v| v.as_str()),
        Some("2.0")
    );
    assert!(response.get("result").is_some());
    println!("✅ Echo tool test passed");

    println!("🎉 All client tests passed!");
    Ok(())
}

async fn send_and_receive(
    client: &mut UnixTransport,
    request: Value,
    msg_id: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let request_str = request.to_string();
    println!("📤 Sending: {}", request_str);

    let message = TransportMessage::new(MessageId::from(msg_id), request_str.into_bytes().into());
    client.send(message).await?;

    let response = timeout(Duration::from_secs(5), client.receive()).await??;
    match response {
        Some(response_msg) => {
            let response_str = String::from_utf8(response_msg.payload.to_vec())?;
            println!("📥 Received: {}", response_str);
            Ok(serde_json::from_str(&response_str)?)
        }
        None => Err("No response received".into()),
    }
}

fn create_test_response(request: &Value) -> Value {
    let method = request
        .get("method")
        .and_then(|m| m.as_str())
        .unwrap_or("unknown");
    let id = request.get("id");

    match method {
        "initialize" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2025-06-18",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "test-server", "version": "1.0.0"}
            }
        }),
        "tools/list" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "tools": [
                    {
                        "name": "echo",
                        "description": "Echo text back",
                        "inputSchema": {
                            "type": "object",
                            "properties": {"text": {"type": "string"}},
                            "required": ["text"]
                        }
                    }
                ]
            }
        }),
        "tools/call" => {
            if let Some(params) = request.get("params") {
                if let Some(tool_name) = params.get("name").and_then(|n| n.as_str()) {
                    match tool_name {
                        "echo" => {
                            let text = params
                                .get("arguments")
                                .and_then(|a| a.get("text"))
                                .and_then(|t| t.as_str())
                                .unwrap_or("(empty)");
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
                        _ => json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": {"code": -32601, "message": "Tool not found"}
                        }),
                    }
                } else {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {"code": -32602, "message": "Invalid params"}
                    })
                }
            } else {
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {"code": -32602, "message": "Invalid params"}
                })
            }
        }
        _ => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32601, "message": "Method not found"}
        }),
    }
}
