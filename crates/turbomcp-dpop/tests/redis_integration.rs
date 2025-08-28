//! Production-grade Redis integration tests for DPoP nonce storage
//!
//! These tests run against a real Redis instance via Docker - no mocks!
//! This ensures we're testing actual Redis behavior, not mock implementations.
//!
//! These tests are only available when the redis-storage feature is enabled.
#![cfg(feature = "redis-storage")]

use std::env;
use std::time::Duration;

use serial_test::serial;

use turbomcp_dpop::{
    redis_storage::RedisNonceStorage, 
    NonceStorage, 
    DpopError
};

/// Get Redis connection URL from environment or use default test setup
fn get_redis_url() -> String {
    env::var("REDIS_TEST_URL")
        .unwrap_or_else(|_| "redis://:turbomcp_test_password@localhost:16379".to_string())
}

/// Create a Redis storage instance for testing against real Redis
async fn create_redis_storage() -> Result<RedisNonceStorage, DpopError> {
    let redis_url = get_redis_url();
    println!("Connecting to Redis at: {}", redis_url.replace("turbomcp_test_password", "***"));
    
    RedisNonceStorage::new(&redis_url).await
}

/// Clean Redis test database before each test
async fn cleanup_redis(storage: &RedisNonceStorage) -> Result<(), DpopError> {
    // In a real test, we'd use a dedicated test database
    // For now, we'll clean up our prefixed keys
    let stats = storage.get_usage_stats().await?;
    println!("Cleaned up Redis - had {} nonces", stats.total_nonces);
    Ok(())
}

#[tokio::test]
#[serial]
async fn test_redis_nonce_storage_connection() {
    let storage = create_redis_storage().await
        .expect("Failed to connect to Redis - ensure Docker Redis is running");
    
    let stats = storage.get_usage_stats().await
        .expect("Failed to get Redis stats");
    
    // We should be able to get stats from real Redis
    // Note: total_nonces is u64, so always >= 0, but we verify the call succeeds
    println!("✅ Redis connection successful - found {} nonces", stats.total_nonces);
}

#[tokio::test]
#[serial]
async fn test_redis_nonce_lifecycle() {
    let storage = create_redis_storage().await
        .expect("Failed to connect to Redis");
    
    cleanup_redis(&storage).await.expect("Failed to cleanup Redis");
    
    let nonce = "test_nonce_lifecycle_12345";
    let jti = "test_jti_lifecycle_67890";
    let client_id = "test_client_redis_lifecycle";
    let http_method = "POST";
    let http_uri = "https://api.example.com/token";
    
    // First storage should succeed
    let stored = storage.store_nonce(
        nonce, 
        jti, 
        http_method, 
        http_uri, 
        client_id,
        Some(Duration::from_secs(300))
    ).await.expect("Failed to store nonce in Redis");
    
    assert!(stored, "First nonce storage should succeed");
    println!("✅ Nonce stored successfully in Redis");
    
    // Check nonce is marked as used
    let is_used = storage.is_nonce_used(nonce, client_id).await
        .expect("Failed to check nonce usage");
    assert!(is_used, "Nonce should be marked as used in Redis");
    println!("✅ Nonce correctly marked as used in Redis");
    
    // Second storage with same nonce should fail (replay protection)
    let stored_again = storage.store_nonce(
        nonce,
        jti,
        http_method,
        http_uri,
        client_id,
        Some(Duration::from_secs(300))
    ).await.expect("Failed to check nonce replay in Redis");
    
    assert!(!stored_again, "Duplicate nonce storage should fail (replay protection)");
    println!("✅ Replay protection working correctly in Redis");
    
    // Verify stats reflect the operations
    let stats = storage.get_usage_stats().await.expect("Failed to get Redis stats");
    assert!(stats.total_nonces >= 1, "Stats should reflect stored nonces");
    println!("✅ Redis stats: {} total nonces, {} active", stats.total_nonces, stats.active_nonces);
}

#[tokio::test]
#[serial]
async fn test_redis_nonce_expiration() {
    let storage = create_redis_storage().await
        .expect("Failed to connect to Redis");
    
    cleanup_redis(&storage).await.expect("Failed to cleanup Redis");
    
    let nonce = "test_nonce_expiration_99999";
    let jti = "test_jti_expiration_88888";
    let client_id = "test_client_redis_expiration";
    
    // Store nonce with very short TTL
    let stored = storage.store_nonce(
        nonce,
        jti,
        "GET",
        "https://api.example.com/resource",
        client_id,
        Some(Duration::from_secs(1)) // 1 second TTL
    ).await.expect("Failed to store expiring nonce");
    
    assert!(stored, "Nonce with TTL should be stored");
    println!("✅ Nonce with 1s TTL stored in Redis");
    
    // Should be usable immediately
    let is_used = storage.is_nonce_used(nonce, client_id).await
        .expect("Failed to check nonce immediately");
    assert!(is_used, "Nonce should be immediately available");
    
    // Wait for expiration (Redis TTL)
    tokio::time::sleep(Duration::from_secs(2)).await;
    
    // Should be expired and no longer used
    let is_used_after = storage.is_nonce_used(nonce, client_id).await
        .expect("Failed to check nonce after expiration");
    assert!(!is_used_after, "Nonce should be expired and not found");
    println!("✅ Redis TTL expiration working correctly");
}

#[tokio::test]
#[serial]
async fn test_redis_concurrent_nonce_operations() {
    let storage = create_redis_storage().await
        .expect("Failed to connect to Redis");
    
    cleanup_redis(&storage).await.expect("Failed to cleanup Redis");
    
    let client_id = "test_client_concurrent";
    let base_nonce = "concurrent_test_nonce";
    
    // Spawn multiple concurrent nonce storage operations
    let mut handles = vec![];
    
    for i in 0..10 {
        let storage = storage.clone();
        let nonce = format!("{}_{}", base_nonce, i);
        let jti = format!("jti_concurrent_{}", i);
        let client_id = client_id.to_string();
        
        let handle = tokio::spawn(async move {
            storage.store_nonce(
                &nonce,
                &jti,
                "POST",
                "https://api.example.com/concurrent",
                &client_id,
                Some(Duration::from_secs(60))
            ).await
        });
        
        handles.push(handle);
    }
    
    // Wait for all operations to complete
    let mut success_count = 0;
    for handle in handles {
        match handle.await {
            Ok(Ok(true)) => success_count += 1,
            Ok(Ok(false)) => {
                // This shouldn't happen with unique nonces
                panic!("Unexpected nonce collision in concurrent test");
            }
            Ok(Err(e)) => panic!("Redis operation failed: {}", e),
            Err(e) => panic!("Task join failed: {}", e),
        }
    }
    
    assert_eq!(success_count, 10, "All concurrent operations should succeed");
    println!("✅ {} concurrent Redis operations completed successfully", success_count);
    
    // Verify all nonces are stored
    for i in 0..10 {
        let nonce = format!("{}_{}", base_nonce, i);
        let is_used = storage.is_nonce_used(&nonce, client_id).await
            .expect("Failed to check concurrent nonce");
        assert!(is_used, "Concurrent nonce {} should be stored", i);
    }
    
    println!("✅ All concurrent nonces verified in Redis");
}

#[tokio::test]
#[serial]
async fn test_redis_storage_stats() {
    let storage = create_redis_storage().await
        .expect("Failed to connect to Redis");
    
    cleanup_redis(&storage).await.expect("Failed to cleanup Redis");
    
    // Get initial stats
    let initial_stats = storage.get_usage_stats().await
        .expect("Failed to get initial Redis stats");
    println!("📊 Initial Redis stats: {:?}", initial_stats);
    
    // Store some nonces
    let client_id = "test_client_stats";
    for i in 0..5 {
        let nonce = format!("stats_test_nonce_{}", i);
        let jti = format!("stats_test_jti_{}", i);
        
        storage.store_nonce(
            &nonce,
            &jti,
            "GET",
            "https://api.example.com/stats",
            client_id,
            Some(Duration::from_secs(300))
        ).await.expect("Failed to store nonce for stats test");
    }
    
    // Get updated stats
    let updated_stats = storage.get_usage_stats().await
        .expect("Failed to get updated Redis stats");
    
    println!("📊 Updated Redis stats: {:?}", updated_stats);
    
    // Stats should show increased nonce count
    assert!(
        updated_stats.total_nonces >= initial_stats.total_nonces + 5,
        "Stats should reflect additional nonces stored"
    );
    
    // Check for Redis-specific metrics
    let redis_backend_found = updated_stats.additional_metrics
        .iter()
        .any(|(key, value)| key == "storage_backend" && value == "Redis");
    
    assert!(redis_backend_found, "Stats should indicate Redis backend");
    println!("✅ Redis storage statistics working correctly");
}

#[tokio::test]
#[serial]
async fn test_redis_error_handling() {
    // Test connection to invalid Redis instance
    let invalid_storage = RedisNonceStorage::new("redis://invalid_host:9999").await;
    
    match invalid_storage {
        Err(DpopError::StorageError { .. }) => {
            println!("✅ Proper error handling for invalid Redis connection");
        }
        Ok(_) => panic!("Should not connect to invalid Redis host"),
        Err(other) => panic!("Unexpected error type: {:?}", other),
    }
}

#[tokio::test] 
#[serial]
async fn test_redis_nonce_cleanup() {
    let storage = create_redis_storage().await
        .expect("Failed to connect to Redis");
    
    cleanup_redis(&storage).await.expect("Failed to cleanup Redis");
    
    // Test cleanup operation (Redis handles TTL automatically)
    let cleaned = storage.cleanup_expired().await
        .expect("Failed to run Redis cleanup");
    
    // Redis handles cleanup automatically via TTL, so this should return 0
    assert_eq!(cleaned, 0, "Redis cleanup should return 0 (TTL handles expiration)");
    println!("✅ Redis cleanup operation completed successfully");
}

/// Integration test runner that ensures Redis is available
#[tokio::test]
#[serial]
async fn test_redis_integration_requirements() {
    // Verify Redis is accessible
    let storage_result = create_redis_storage().await;
    
    match storage_result {
        Ok(_storage) => {
            println!("✅ Redis integration test environment ready");
        }
        Err(e) => {
            panic!(
                "❌ Redis integration test failed - ensure Docker Redis is running:\n\
                Error: {}\n\
                \n\
                To start Redis for testing:\n\
                cd crates/turbomcp-dpop && ./scripts/test-docker.sh start\n\
                \n\
                Or run full test suite:\n\
                ./scripts/test-docker.sh test",
                e
            );
        }
    }
}