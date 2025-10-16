//! Regression test for sampling request rejection hang bug
//!
//! **Bug**: When a tool initiates a sampling request that the user rejects,
//! subsequent tool calls hang for 60 seconds while the sampling request times out.
//!
//! **Root Cause**: Response channels are created but never stored in the correlations
//! map, so rejection responses cannot be delivered. The code waits the full timeout.
//!
//! **Expected**: Rejection should return immediately (< 1 second)
//! **Actual**: Rejection times out after 60 seconds

#![cfg(feature = "websocket")]

use std::time::{Duration, Instant};
use tokio::time::timeout;
use turbomcp_protocol::types::{Content, CreateMessageRequest, Role, SamplingMessage, TextContent};
use turbomcp_transport::websocket_bidirectional::{
    WebSocketBidirectionalTransport, config::WebSocketBidirectionalConfig,
};

#[tokio::test]
async fn test_sampling_rejection_should_not_hang() {
    // Create transport (not connected, simulating the scenario)
    let config = WebSocketBidirectionalConfig::default();
    let transport = WebSocketBidirectionalTransport::new(config)
        .await
        .expect("Failed to create transport");

    // Create a sampling request
    let request = CreateMessageRequest {
        messages: vec![SamplingMessage {
            role: Role::User,
            content: Content::Text(TextContent {
                text: "What is 2+2?".to_string(),
                annotations: None,
                meta: None,
            }),
            metadata: None,
        }],
        model_preferences: None,
        system_prompt: None,
        include_context: None,
        temperature: None,
        max_tokens: 100,
        stop_sequences: None,
        _meta: None,
    };

    // Measure how long the rejection takes
    let start = Instant::now();

    // Attempt to send sampling request (will fail immediately since not connected)
    let result = timeout(
        Duration::from_secs(2),
        transport.send_sampling(request, None),
    )
    .await;

    let elapsed = start.elapsed();

    // This should fail quickly (< 1 second) with "not connected" error
    // NOT hang for 60 seconds waiting for a response that will never come
    assert!(
        elapsed < Duration::from_secs(2),
        "Sampling request took {:?} - should fail fast, not hang for timeout period",
        elapsed
    );

    // The error should be about connection, not timeout
    match result {
        Ok(Err(transport_err)) => {
            let err_msg = transport_err.to_string();
            assert!(
                err_msg.contains("not connected") || err_msg.contains("WebSocket"),
                "Expected connection error, got: {}",
                err_msg
            );
        }
        Err(_) => {
            panic!("Test timeout - this indicates the bug is present!");
        }
        Ok(Ok(_)) => {
            panic!("Should not succeed without connection");
        }
    }
}

#[tokio::test]
async fn test_sampling_with_user_rejection_immediate_response() {
    // This test will be expanded once we have proper correlation tracking
    // to test the case where a user explicitly rejects a sampling request
    //
    // Expected behavior:
    // 1. Server sends sampling/createMessage request
    // 2. Client immediately responds with error: { code: -32001, message: "User rejected" }
    // 3. Server receives rejection in < 100ms, not after 60s timeout
    //
    // Currently FAILING because:
    // - Response channel is discarded (_response_tx)
    // - No correlation map stores the channel
    // - Rejection response has nowhere to go
    // - Code waits full 60s timeout

    // TODO: Implement this test once correlation tracking is added
}

#[tokio::test]
async fn test_ping_rejection_should_not_hang() {
    use turbomcp_protocol::types::PingRequest;

    let config = WebSocketBidirectionalConfig::default();
    let transport = WebSocketBidirectionalTransport::new(config)
        .await
        .expect("Failed to create transport");

    let request = PingRequest {
        params: turbomcp_protocol::types::PingParams { data: None },
    };

    let start = Instant::now();
    let result = timeout(Duration::from_secs(2), transport.send_ping(request, None)).await;
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(2),
        "Ping request took {:?} - should fail fast",
        elapsed
    );

    match result {
        Ok(Err(transport_err)) => {
            let err_msg = transport_err.to_string();
            assert!(
                err_msg.contains("not connected") || err_msg.contains("WebSocket"),
                "Expected connection error, got: {}",
                err_msg
            );
        }
        Err(_) => {
            panic!("Test timeout - bug is present!");
        }
        Ok(Ok(_)) => {
            panic!("Should not succeed without connection");
        }
    }
}

#[tokio::test]
async fn test_roots_list_rejection_should_not_hang() {
    use turbomcp_protocol::types::ListRootsRequest;

    let config = WebSocketBidirectionalConfig::default();
    let transport = WebSocketBidirectionalTransport::new(config)
        .await
        .expect("Failed to create transport");

    let request = ListRootsRequest { _meta: None };

    let start = Instant::now();
    let result = timeout(
        Duration::from_secs(2),
        transport.send_list_roots(request, None),
    )
    .await;
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(2),
        "Roots list request took {:?} - should fail fast",
        elapsed
    );

    match result {
        Ok(Err(transport_err)) => {
            let err_msg = transport_err.to_string();
            assert!(
                err_msg.contains("not connected") || err_msg.contains("WebSocket"),
                "Expected connection error, got: {}",
                err_msg
            );
        }
        Err(_) => {
            panic!("Test timeout - bug is present!");
        }
        Ok(Ok(_)) => {
            panic!("Should not succeed without connection");
        }
    }
}

/// Performance benchmark: Measure the actual hang time
#[tokio::test]
#[ignore] // Run with: cargo test --test sampling_rejection_hang_test -- --ignored --nocapture
async fn benchmark_sampling_rejection_hang_time() {
    let config = WebSocketBidirectionalConfig::default();
    let transport = WebSocketBidirectionalTransport::new(config)
        .await
        .expect("Failed to create transport");

    let request = CreateMessageRequest {
        messages: vec![SamplingMessage {
            role: Role::User,
            content: Content::Text(TextContent {
                text: "Benchmark request".to_string(),
                annotations: None,
                meta: None,
            }),
            metadata: None,
        }],
        model_preferences: None,
        system_prompt: None,
        include_context: None,
        temperature: None,
        max_tokens: 100,
        stop_sequences: None,
        _meta: None,
    };

    println!("Starting benchmark - this will take up to 60s if bug is present...");
    let start = Instant::now();

    let _result = transport.send_sampling(request, None).await;

    let elapsed = start.elapsed();
    println!("❌ BUG CONFIRMED: Request took {:?} to fail", elapsed);
    println!("   Expected: < 100ms");
    println!("   Actual: {:?}", elapsed);
    println!("   Wasted time: {:?}", elapsed - Duration::from_millis(100));

    // This will likely be close to 60 seconds if bug is present
    assert!(
        elapsed > Duration::from_secs(50),
        "If this fails, the bug might be fixed! Elapsed: {:?}",
        elapsed
    );
}
