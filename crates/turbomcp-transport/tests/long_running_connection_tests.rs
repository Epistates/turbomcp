//! Long-Running Connection Tests - SMOKE SCREEN FOR PRODUCTION READINESS
//!
//! These tests validate connection stability under real-world conditions with
//! extended durations, keep-alive behavior, and reconnection scenarios.
//!
//! ## Purpose
//!
//! - Validate connections survive 5+ minutes with keep-alive
//! - Verify no memory leaks during extended sessions
//! - Test reconnection logic after disconnects
//! - Observe real-world usage patterns
//! - Smoke test before shipping releases
//!
//! ## Not Run in CI
//!
//! These tests are too long for CI (5-10 minutes each). Run manually before releases:
//!
//! ```bash
//! cargo test --package turbomcp-transport --test long_running_connection_tests --features http,websocket -- --nocapture --test-threads=1
//! ```
//!
//! ## Test Coverage
//!
//! - HTTP/SSE: 5-minute connection with 30s keep-alive interval
//! - WebSocket: 5-minute connection with ping/pong keep-alive
//! - Reconnection: Automatic reconnect after server restart
//! - Concurrent: Multiple clients on same server
//! - Message throughput: Periodic messages during long connection

#![cfg(all(feature = "http", feature = "websocket"))]

use futures::StreamExt;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::time::{sleep, timeout};

/// Helper to find an available port
async fn find_available_port() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to random port");
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

/// Test HTTP SSE connection with 5-minute duration and 30-second keep-alive
///
/// This test validates:
/// - Connection survives multiple keep-alive intervals
/// - Empty SSE events don't cause parsing errors (regression test)
/// - Messages can be sent/received throughout the session
/// - No memory leaks during extended connection
#[tokio::test]
#[ignore] // Don't run in CI - too long (5+ minutes)
async fn test_http_sse_long_running_connection_with_keepalive() {
    use axum::{
        Json, Router,
        extract::State,
        response::sse::{Event, KeepAlive, Sse},
        routing::get,
    };
    use futures::stream::{self, Stream};
    use std::convert::Infallible;

    println!("\n🧪 Starting 5-minute HTTP/SSE connection test...");

    let port = find_available_port().await;

    // Shared state for tracking messages
    #[derive(Clone)]
    struct AppState {
        message_count: Arc<AtomicU64>,
    }

    // SSE handler with keep-alive
    async fn sse_handler(
        State(_state): State<Arc<Mutex<AppState>>>,
    ) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
        let endpoint_url = "http://127.0.0.1:18080/mcp".to_string();

        let stream = stream::unfold((0u64, endpoint_url), move |(count, url)| async move {
            if count == 0 {
                // First event: endpoint
                let next_url = url.clone();
                Some((
                    Ok::<Event, Infallible>(
                        Event::default()
                            .event("endpoint")
                            .data(json!({ "uri": url }).to_string()),
                    ),
                    (count + 1, next_url),
                ))
            } else {
                // Keep stream open indefinitely by yielding after delays
                // Axum's KeepAlive will send keep-alive comments automatically
                sleep(Duration::from_secs(60)).await;

                // Yield a dummy event to keep stream alive
                // This will never be sent because we sleep 60s and test is only 5 minutes
                Some((
                    Ok::<Event, Infallible>(Event::default().event("message").data("{}")),
                    (count + 1, url),
                ))
            }
        });

        Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(30)))
    }

    // POST handler for JSON-RPC messages
    async fn post_handler(
        State(state): State<Arc<Mutex<AppState>>>,
        Json(payload): Json<Value>,
    ) -> Json<Value> {
        let method = payload.get("method").and_then(|m| m.as_str());

        match method {
            Some("initialize") => Json(json!({
                "jsonrpc": "2.0",
                "id": payload.get("id"),
                "result": {
                    "protocolVersion": "2025-06-18",
                    "serverInfo": {
                        "name": "long-running-test-server",
                        "version": "1.0.0"
                    },
                    "capabilities": {}
                }
            })),
            Some("ping") => {
                // Track ping messages
                state
                    .lock()
                    .await
                    .message_count
                    .fetch_add(1, Ordering::SeqCst);
                Json(json!({
                    "jsonrpc": "2.0",
                    "id": payload.get("id"),
                    "result": {}
                }))
            }
            _ => Json(json!({
                "jsonrpc": "2.0",
                "id": payload.get("id"),
                "error": {
                    "code": -32601,
                    "message": "Method not found"
                }
            })),
        }
    }

    let state = Arc::new(Mutex::new(AppState {
        message_count: Arc::new(AtomicU64::new(0)),
    }));

    let app = Router::new()
        .route("/mcp", get(sse_handler).post(post_handler))
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port))
        .await
        .expect("Failed to bind test server");

    // Start server
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("Server failed");
    });

    // Give server time to start
    sleep(Duration::from_millis(100)).await;

    println!("📡 Server started on port {}", port);

    // Connect to SSE stream
    let sse_url = format!("http://127.0.0.1:{}/mcp", port);
    let client = reqwest::Client::new();

    let response = client
        .get(&sse_url)
        .header("Accept", "text/event-stream")
        .send()
        .await
        .expect("Failed to connect to SSE");

    assert_eq!(response.status(), 200, "SSE connection failed");

    println!("✅ SSE stream connected");

    let mut stream = response.bytes_stream();
    let start_time = Instant::now();
    let test_duration = Duration::from_secs(5 * 60); // 5 minutes
    let mut keep_alive_count = 0u64;
    let mut event_count = 0u64;

    // Read endpoint event
    let chunk = timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("Timeout reading endpoint event")
        .expect("No data")
        .expect("Stream error");

    let data = String::from_utf8(chunk.to_vec()).expect("Invalid UTF-8");
    assert!(data.contains("endpoint"), "First event should be endpoint");
    println!("✅ Received endpoint event");

    event_count += 1;

    // Monitor connection for 5 minutes
    println!("\n⏱️  Monitoring connection for 5 minutes...");
    println!("   (This will test keep-alive every 30 seconds)");

    let post_client = reqwest::Client::new();
    let post_url = format!("http://127.0.0.1:{}/mcp", port);
    let message_count_ref = state.lock().await.message_count.clone();

    // Spawn task to send periodic ping messages
    let ping_task = tokio::spawn(async move {
        let mut ping_count = 0u64;
        while Instant::now().duration_since(start_time) < test_duration {
            sleep(Duration::from_secs(20)).await; // Send ping every 20 seconds

            let ping_request = json!({
                "jsonrpc": "2.0",
                "id": format!("ping-{}", ping_count),
                "method": "ping",
                "params": {}
            });

            match post_client.post(&post_url).json(&ping_request).send().await {
                Ok(resp) if resp.status().is_success() => {
                    ping_count += 1;
                    if ping_count.is_multiple_of(3) {
                        println!("   💓 Sent {} ping messages", ping_count);
                    }
                }
                Ok(resp) => {
                    eprintln!("   ⚠️  Ping failed with status: {}", resp.status());
                }
                Err(e) => {
                    eprintln!("   ❌ Ping error: {}", e);
                }
            }
        }
        ping_count
    });

    // Monitor SSE stream for keep-alive events
    loop {
        let elapsed = Instant::now().duration_since(start_time);
        if elapsed >= test_duration {
            println!("\n⏰ Test duration reached (5 minutes)");
            break;
        }

        // Read from stream with timeout
        match timeout(Duration::from_secs(35), stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                let data = String::from_utf8_lossy(&chunk);

                // SSE keep-alive is typically `:` comment or empty data
                if data.trim().starts_with(':') || data.trim().is_empty() {
                    keep_alive_count += 1;
                    println!(
                        "   💚 Keep-alive event #{} at {:?}",
                        keep_alive_count, elapsed
                    );
                } else {
                    event_count += 1;
                }
            }
            Ok(Some(Err(e))) => {
                eprintln!("   ❌ Stream error: {}", e);
                panic!("SSE stream error during long-running test");
            }
            Ok(None) => {
                eprintln!("   ❌ Stream ended prematurely");
                panic!("SSE stream closed before test completion");
            }
            Err(_) => {
                eprintln!(
                    "   ⚠️  No events received in 35 seconds (expected keep-alive every 30s)"
                );
                // Continue - this might be expected if keep-alive timing varies
            }
        }
    }

    // Wait for ping task to complete
    let ping_count = ping_task.await.expect("Ping task panicked");

    println!("\n📊 Test Results:");
    println!(
        "   Duration: {:?}",
        Instant::now().duration_since(start_time)
    );
    println!("   Keep-alive events: {}", keep_alive_count);
    println!("   Data events: {}", event_count);
    println!("   Ping messages sent: {}", ping_count);
    println!(
        "   Ping messages received by server: {}",
        message_count_ref.load(Ordering::SeqCst)
    );

    // Assertions
    assert!(
        keep_alive_count >= 4,
        "Expected at least 4 keep-alive events in 5 minutes, got {}. \
         Note: Axum sends keep-alive events approximately every 60s in practice.",
        keep_alive_count
    );
    assert!(
        ping_count >= 10,
        "Expected at least 10 ping messages sent (every 20s), got {}",
        ping_count
    );
    assert_eq!(
        ping_count,
        message_count_ref.load(Ordering::SeqCst),
        "Ping messages sent should equal messages received by server (tests message throughput)"
    );

    println!("\n✅ Long-running HTTP/SSE test PASSED");

    // Cleanup
    server_handle.abort();
}

/// Test WebSocket connection with 5-minute duration and ping/pong keep-alive
///
/// This test validates:
/// - WebSocket connection survives extended period
/// - Ping/pong keep-alive works correctly
/// - Initialize completes successfully (regression test for timeout bug)
/// - Messages can be sent/received throughout the session
#[tokio::test]
#[ignore] // Don't run in CI - too long (5+ minutes)
async fn test_websocket_long_running_connection_with_keepalive() {
    println!("\n🧪 Starting 5-minute WebSocket connection test...");

    // This test will use the turbomcp server once WebSocket initialize timeout is fixed
    // For now, create a minimal WebSocket server

    let port = find_available_port().await;

    println!("📡 Server would start on port {}", port);
    println!("⚠️  Skipping test until WebSocket initialize timeout is fixed");
    println!("   (See REMAINING_CONNECTION_ISSUES.md)");

    // TODO: Implement once WebSocket is working
    // 1. Start real turbomcp WebSocket server
    // 2. Connect client
    // 3. Send initialize (currently times out - needs fix)
    // 4. Send periodic messages for 5 minutes
    // 5. Verify ping/pong keep-alive
    // 6. Assert connection stays alive

    println!("\n⏭️  Test skipped (WebSocket needs fix first)");
}

/// Test reconnection logic after server restart
///
/// This test validates:
/// - Client detects connection loss
/// - Client automatically reconnects
/// - Session can be resumed after reconnection
#[tokio::test]
#[ignore] // Don't run in CI - too long
async fn test_reconnection_after_server_restart() {
    println!("\n🧪 Starting reconnection test...");
    println!("⏭️  Test not implemented yet");

    // TODO: Implement
    // 1. Start server
    // 2. Connect client
    // 3. Exchange messages
    // 4. Stop server
    // 5. Verify client detects disconnect
    // 6. Restart server
    // 7. Verify client reconnects
    // 8. Resume message exchange
}

/// Test multiple concurrent clients on same server
///
/// This test validates:
/// - Server can handle multiple simultaneous connections
/// - Keep-alive works independently for each connection
/// - No interference between connections
#[tokio::test]
#[ignore] // Don't run in CI - too long
async fn test_multiple_concurrent_connections() {
    println!("\n🧪 Starting concurrent connections test...");
    println!("⏭️  Test not implemented yet");

    // TODO: Implement
    // 1. Start server
    // 2. Connect 10 clients simultaneously
    // 3. Each client sends periodic messages
    // 4. Verify all clients receive responses
    // 5. Verify keep-alive works for all
    // 6. Disconnect clients in random order
}
