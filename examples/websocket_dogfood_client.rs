//! WebSocket Dogfood Client - Test Initialize
//!
//! This example connects to the dogfood server and sends an initialize request
//! to observe if/when it times out.
//!
//! ## Usage
//!
//! ```bash
//! # In one terminal, start the server:
//! RUST_LOG=debug,turbomcp_server::runtime::websocket=trace \
//!   cargo run --example websocket_dogfood_server --features websocket
//!
//! # In another terminal, run this client:
//! RUST_LOG=debug cargo run --example websocket_dogfood_client --features websocket
//! ```

use futures::{SinkExt, StreamExt};
use serde_json::json;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    println!("\n🧪 WebSocket Dogfood Client Starting...");
    println!("   Connecting to: ws://127.0.0.1:8080/ws");
    println!("   Timeout: 30 seconds\n");

    // Connect to WebSocket server
    tracing::info!("Connecting to WebSocket server...");
    let (ws_stream, _) = match timeout(
        Duration::from_secs(5),
        connect_async("ws://127.0.0.1:8080/ws"),
    )
    .await
    {
        Ok(Ok(stream)) => stream,
        Ok(Err(e)) => {
            eprintln!("❌ Connection failed: {}", e);
            eprintln!("   Make sure the server is running!");
            return Err(e.into());
        }
        Err(_) => {
            eprintln!("❌ Connection timeout");
            return Err("Connection timeout".into());
        }
    };

    println!("✅ WebSocket connected\n");

    let (mut write, mut read) = ws_stream.split();

    // Send initialize request
    let init_request = json!({
        "jsonrpc": "2.0",
        "id": "init-1",
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {
                "name": "dogfood-client",
                "version": "1.0.0"
            }
        }
    });

    let init_str = serde_json::to_string(&init_request)?;

    println!("📤 Sending initialize request:");
    println!("   {}\n", init_str);

    tracing::info!("Sending initialize request");
    write.send(Message::Text(init_str.into())).await?;

    println!("⏳ Waiting for response (30s timeout)...");
    println!("   (If this times out, check server logs for missing log messages)\n");

    // Wait for response with timeout
    match timeout(Duration::from_secs(30), read.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => {
            println!("✅ Received response:");
            println!("   {}\n", text);

            // Parse response
            match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(json) => {
                    if let Some(result) = json.get("result") {
                        println!("✅ Initialize succeeded!");
                        println!("   Protocol: {}", result["protocolVersion"]);
                        println!("   Server: {} v{}",
                            result["serverInfo"]["name"],
                            result["serverInfo"]["version"]
                        );
                    } else if let Some(error) = json.get("error") {
                        eprintln!("❌ Initialize failed with error:");
                        eprintln!("   {}", error);
                    }
                }
                Err(e) => {
                    eprintln!("⚠️  Response is not valid JSON: {}", e);
                    eprintln!("   Raw response: {}", text);
                }
            }
        }
        Ok(Some(Ok(msg))) => {
            eprintln!("⚠️  Received unexpected message type: {:?}", msg);
        }
        Ok(Some(Err(e))) => {
            eprintln!("❌ WebSocket error: {}", e);
        }
        Ok(None) => {
            eprintln!("❌ Connection closed before response");
        }
        Err(_) => {
            eprintln!("❌ Timeout waiting for initialize response (30s)");
            eprintln!("\n🔍 This is the bug! Check server logs to see where it broke:");
            eprintln!("   - Did 'Received WebSocket message' appear?");
            eprintln!("   - Did 'Handling WebSocket request: method=initialize' appear?");
            eprintln!("   - Did 'Calling handler' appear?");
            eprintln!("   - Did 'Handler returned response' appear?");
            eprintln!("   - Did 'Sending WebSocket response' appear?");
            eprintln!("\n   The FIRST missing log tells you where the code breaks!");
        }
    }

    // Test echo if initialize succeeded
    println!("\n📤 Testing echo tool...");

    let echo_request = json!({
        "jsonrpc": "2.0",
        "id": "echo-1",
        "method": "tools/call",
        "params": {
            "name": "echo",
            "arguments": {
                "message": "Hello from dogfood client!"
            }
        }
    });

    let echo_str = serde_json::to_string(&echo_request)?;
    write.send(Message::Text(echo_str.into())).await?;

    match timeout(Duration::from_secs(5), read.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => {
            println!("✅ Echo response: {}", text);
        }
        _ => {
            eprintln!("⚠️  No echo response (might be expected if initialize failed)");
        }
    }

    println!("\n✅ Dogfood client test complete");

    Ok(())
}
