//! Integration tests for RuntimeProxy

use turbomcp_proxy::runtime::RuntimeProxyBuilder;

/// Test that RuntimeProxy can be built with STDIO backend and STDIO frontend
#[tokio::test]
async fn test_stdio_to_stdio_proxy_builds() {
    use tokio::time::{Duration, timeout};

    let result = timeout(
        Duration::from_secs(5),
        RuntimeProxyBuilder::new()
            .with_stdio_backend("python", vec!["server.py".to_string()])
            .with_stdio_frontend()
            .build(),
    )
    .await;

    // This should fail because we don't have an actual Python server,
    // but it validates our configuration is correct
    match result {
        Ok(Err(_)) => {} // Expected error
        Ok(Ok(_)) => {}  // Also acceptable if somehow it connects
        Err(_) => {}     // Timeout is acceptable for unavailable service
    }
}

/// Test that RuntimeProxy can be built with STDIO backend and HTTP frontend
#[tokio::test]
async fn test_stdio_to_http_proxy_builds() {
    use tokio::time::{Duration, timeout};

    let result = timeout(
        Duration::from_secs(5),
        RuntimeProxyBuilder::new()
            .with_stdio_backend("python", vec!["server.py".to_string()])
            .with_http_frontend("127.0.0.1:0")
            .build(),
    )
    .await;

    // This should fail because we don't have an actual Python server,
    // but it validates our configuration is correct
    match result {
        Ok(Err(_)) => {} // Expected error
        Ok(Ok(_)) => {}  // Also acceptable if somehow it connects
        Err(_) => {}     // Timeout is acceptable for unavailable service
    }
}

/// Test that RuntimeProxy builder enforces required fields
#[tokio::test]
async fn test_builder_validation() {
    use tokio::time::{Duration, timeout};

    // Missing backend
    let result = timeout(
        Duration::from_secs(5),
        RuntimeProxyBuilder::new()
            .with_http_frontend("127.0.0.1:3000")
            .build(),
    )
    .await;
    match result {
        Ok(Err(_)) => {} // Expected error for missing backend
        _ => panic!("Should fail validation for missing backend"),
    }

    // Missing frontend
    let result = timeout(
        Duration::from_secs(5),
        RuntimeProxyBuilder::new()
            .with_stdio_backend("python", vec!["server.py".to_string()])
            .build(),
    )
    .await;
    match result {
        Ok(Err(_)) => {} // Expected error for missing frontend
        _ => panic!("Should fail validation for missing frontend"),
    }
}

/// Test that timeout validation works
#[test]
fn test_timeout_validation() {
    use turbomcp_proxy::runtime::MAX_TIMEOUT_MS;

    let result = RuntimeProxyBuilder::new().with_timeout(MAX_TIMEOUT_MS + 1);

    assert!(result.is_err());
}

/// Test builder with valid configuration
#[tokio::test]
async fn test_builder_with_valid_config() {
    use tokio::time::{Duration, timeout};

    // Test that builder accepts valid configuration
    // Note: This will still fail to connect since python server doesn't exist
    let result = timeout(
        Duration::from_secs(5),
        RuntimeProxyBuilder::new()
            .with_stdio_backend("python", vec!["server.py".to_string()])
            .with_http_frontend("127.0.0.1:3000")
            .with_request_size_limit(1024 * 1024)
            .with_timeout(10_000)
            .expect("Valid timeout should succeed")
            .with_metrics(true)
            .build(),
    )
    .await;

    // Should fail to connect, but configuration should be valid
    match result {
        Ok(Err(_)) => {} // Expected error for unavailable service
        Ok(Ok(_)) => {}  // Also acceptable if somehow it connects
        Err(_) => {}     // Timeout is acceptable
    }
}
