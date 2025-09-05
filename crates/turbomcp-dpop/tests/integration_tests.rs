//! Integration tests for TurboMCP DPoP implementation
//!
//! These tests validate the complete DPoP flow including key generation,
//! proof creation, validation, and integration with the transport layer.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use turbomcp_dpop::{
    DpopAlgorithm, DpopError, DpopKeyManager, DpopProofGenerator, MemoryNonceTracker, NonceTracker,
    Result,
};

/// Test key generation for all supported algorithms
#[tokio::test]
async fn test_key_generation_all_algorithms() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);

    // Test ES256 (ECDSA P-256)
    let es256_key = key_manager.generate_key_pair(DpopAlgorithm::ES256).await?;
    assert_eq!(es256_key.algorithm, DpopAlgorithm::ES256);
    assert!(!es256_key.thumbprint.is_empty());
    assert!(!es256_key.is_expired());

    // Test RS256 (RSA with PKCS#1 v1.5)
    let rs256_key = key_manager.generate_key_pair(DpopAlgorithm::RS256).await?;
    assert_eq!(rs256_key.algorithm, DpopAlgorithm::RS256);
    assert!(!rs256_key.thumbprint.is_empty());
    assert_ne!(rs256_key.thumbprint, es256_key.thumbprint);

    // Test PS256 (RSA with PSS)
    let ps256_key = key_manager.generate_key_pair(DpopAlgorithm::PS256).await?;
    assert_eq!(ps256_key.algorithm, DpopAlgorithm::PS256);
    assert!(!ps256_key.thumbprint.is_empty());

    Ok(())
}

/// Test complete DPoP proof generation and validation flow
#[tokio::test]
async fn test_dpop_proof_flow() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let proof_gen = DpopProofGenerator::new(key_manager);

    let method = "POST";
    let uri = "https://api.example.com/oauth/token";
    let access_token = "test-access-token-123";

    // Generate proof
    let proof = proof_gen
        .generate_proof(method, uri, Some(access_token))
        .await?;

    // Validate proof structure
    proof.validate_structure()?;

    // Validate proof cryptographically
    let validation_result = proof_gen
        .validate_proof(&proof, method, uri, Some(access_token))
        .await?;

    assert!(validation_result.valid);
    assert_eq!(validation_result.key_algorithm, DpopAlgorithm::ES256);

    Ok(())
}

/// Test DPoP proof validation with wrong parameters fails appropriately
#[tokio::test]
async fn test_dpop_proof_validation_failures() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let proof_gen = DpopProofGenerator::new(key_manager);

    let method = "POST";
    let uri = "https://api.example.com/oauth/token";

    // Generate a valid proof
    let proof = proof_gen.generate_proof(method, uri, None).await?;

    // Test wrong HTTP method
    let wrong_method_result = proof_gen.validate_proof(&proof, "GET", uri, None).await;
    assert!(wrong_method_result.is_err());
    assert!(matches!(
        wrong_method_result.unwrap_err(),
        DpopError::HttpBindingFailed { .. }
    ));

    // Test wrong URI
    let wrong_uri_result = proof_gen
        .validate_proof(&proof, method, "https://other.example.com/token", None)
        .await;
    assert!(wrong_uri_result.is_err());
    assert!(matches!(
        wrong_uri_result.unwrap_err(),
        DpopError::HttpBindingFailed { .. }
    ));

    Ok(())
}

/// Test replay attack prevention
#[tokio::test]
async fn test_replay_attack_prevention() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let nonce_tracker = Arc::new(MemoryNonceTracker::new());
    let proof_gen = DpopProofGenerator::with_nonce_tracker(key_manager, nonce_tracker);

    let method = "POST";
    let uri = "https://api.example.com/oauth/token";

    // Generate proof
    let proof = proof_gen.generate_proof(method, uri, None).await?;

    // First validation should succeed
    let first_validation = proof_gen.validate_proof(&proof, method, uri, None).await?;
    assert!(first_validation.valid);

    // Second validation of same proof should fail (replay attack)
    let replay_result = proof_gen.validate_proof(&proof, method, uri, None).await;
    assert!(replay_result.is_err());
    assert!(matches!(
        replay_result.unwrap_err(),
        DpopError::ReplayAttackDetected { .. }
    ));

    Ok(())
}

/// Test access token hash validation
#[tokio::test]
async fn test_access_token_hash_validation() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let proof_gen = DpopProofGenerator::new(key_manager.clone());

    let method = "GET";
    let uri = "https://api.example.com/protected";
    let access_token = "test-access-token-456";

    // Generate proof with access token
    let proof = proof_gen
        .generate_proof(method, uri, Some(access_token))
        .await?;

    // Validate with correct token
    let correct_validation = proof_gen
        .validate_proof(&proof, method, uri, Some(access_token))
        .await?;
    assert!(correct_validation.valid);

    // Generate a separate proof with different access token for wrong token test
    let proof_wrong_token = proof_gen
        .generate_proof(method, uri, Some("wrong-token"))
        .await?;

    // Validate proof generated with wrong token using correct token should fail
    let wrong_token_result = proof_gen
        .validate_proof(&proof_wrong_token, method, uri, Some(access_token))
        .await;
    assert!(wrong_token_result.is_err());
    assert!(matches!(
        wrong_token_result.unwrap_err(),
        DpopError::AccessTokenHashFailed { .. }
    ));

    // Generate a proof without access token for missing token test
    let proof_no_token = proof_gen.generate_proof(method, uri, None).await?;

    // Validate proof without token hash using access token should succeed
    let result_no_hash = proof_gen
        .validate_proof(&proof_no_token, method, uri, Some(access_token))
        .await?;
    assert!(result_no_hash.valid);

    // Use separate generator for missing token test to avoid nonce conflicts
    let proof_gen_2 = DpopProofGenerator::new(key_manager.clone());
    let proof_with_token = proof_gen_2
        .generate_proof(method, uri, Some(access_token))
        .await?;

    // Validate proof with token hash but no provided token should fail
    let missing_token_result = proof_gen_2
        .validate_proof(&proof_with_token, method, uri, None)
        .await;
    assert!(missing_token_result.is_err());
    assert!(matches!(
        missing_token_result.unwrap_err(),
        DpopError::AccessTokenHashFailed { .. }
    ));

    Ok(())
}

/// Test key rotation functionality
#[tokio::test]
async fn test_key_rotation() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);

    // Generate initial key
    let original_key = key_manager.generate_key_pair(DpopAlgorithm::ES256).await?;
    let original_id = original_key.id.clone();

    // Rotate the key
    let rotated_key = key_manager.rotate_key_pair(&original_id).await?;

    // Verify rotation properties
    assert_ne!(rotated_key.id, original_key.id);
    assert_ne!(rotated_key.thumbprint, original_key.thumbprint);
    assert_eq!(rotated_key.algorithm, original_key.algorithm);
    assert_eq!(rotated_key.metadata.rotation_generation, 1);

    // Original key should still be retrievable (but expired)
    let retrieved_original = key_manager.get_key_pair(&original_id).await?;
    assert!(retrieved_original.is_some());
    assert!(retrieved_original.unwrap().is_expired());

    Ok(())
}

/// Test key lookup by thumbprint
#[tokio::test]
async fn test_key_thumbprint_lookup() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);

    // Generate multiple keys
    let key1 = key_manager.generate_key_pair(DpopAlgorithm::ES256).await?;
    let key2 = key_manager.generate_key_pair(DpopAlgorithm::RS256).await?;

    // Test thumbprint lookup
    let found_key1 = key_manager
        .get_key_pair_by_thumbprint(&key1.thumbprint)
        .await?;
    assert!(found_key1.is_some());
    assert_eq!(found_key1.unwrap().id, key1.id);

    let found_key2 = key_manager
        .get_key_pair_by_thumbprint(&key2.thumbprint)
        .await?;
    assert!(found_key2.is_some());
    assert_eq!(found_key2.unwrap().id, key2.id);

    // Test non-existent thumbprint
    let not_found = key_manager
        .get_key_pair_by_thumbprint("nonexistent-thumbprint")
        .await?;
    assert!(not_found.is_none());

    Ok(())
}

/// Test expired key cleanup
#[tokio::test]
async fn test_expired_key_cleanup() -> Result<()> {
    use turbomcp_dpop::keys::KeyRotationPolicy;

    // Create key manager with short-lived keys
    let policy = KeyRotationPolicy {
        key_lifetime: Duration::from_millis(100), // Very short for testing
        auto_rotate: true,
        rotation_check_interval: Duration::from_secs(1),
    };

    let storage = Arc::new(turbomcp_dpop::keys::MemoryKeyStorage::new());
    let key_manager = Arc::new(DpopKeyManager::new(storage, policy).await?);

    // Generate a key
    let key = key_manager.generate_key_pair(DpopAlgorithm::ES256).await?;

    // Wait for expiration
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Key should now be expired
    let retrieved_key = key_manager.get_key_pair(&key.id).await?;
    assert!(retrieved_key.is_some());
    assert!(retrieved_key.unwrap().is_expired());

    // Cleanup expired keys
    let cleaned_count = key_manager.cleanup_expired_keys().await?;
    assert_eq!(cleaned_count, 1);

    // Key should no longer exist
    let after_cleanup = key_manager.get_key_pair(&key.id).await?;
    assert!(after_cleanup.is_none());

    Ok(())
}

/// Test nonce tracker functionality
#[tokio::test]
async fn test_nonce_tracker() -> Result<()> {
    let nonce_tracker = MemoryNonceTracker::new();
    let test_nonce = "test-nonce-123";
    let timestamp = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    // Initially nonce should not be used
    assert!(!nonce_tracker.is_nonce_used(test_nonce).await?);

    // Track the nonce
    nonce_tracker.track_nonce(test_nonce, timestamp).await?;

    // Now nonce should be marked as used
    assert!(nonce_tracker.is_nonce_used(test_nonce).await?);

    // Different nonce should still be unused
    assert!(!nonce_tracker.is_nonce_used("other-nonce").await?);

    Ok(())
}

/// Test URI cleaning functionality
#[tokio::test]
async fn test_uri_cleaning() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let proof_gen = DpopProofGenerator::new(key_manager);

    // Generate proof with URI that has query parameters
    let uri_with_query = "https://api.example.com/token?grant_type=client_credentials&scope=read";
    let proof = proof_gen
        .generate_proof("POST", uri_with_query, None)
        .await?;

    // Proof should be valid with clean URI (no query)
    let clean_uri = "https://api.example.com/token";
    let validation_result = proof_gen
        .validate_proof(&proof, "POST", clean_uri, None)
        .await?;

    assert!(validation_result.valid);

    Ok(())
}

/// Test DPoP with different algorithms
#[tokio::test]
async fn test_dpop_different_algorithms() -> Result<()> {
    let algorithms = [
        DpopAlgorithm::ES256,
        DpopAlgorithm::RS256,
        DpopAlgorithm::PS256,
    ];

    for algorithm in algorithms {
        let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
        let key_pair = key_manager.generate_key_pair(algorithm).await?;
        let proof_gen = DpopProofGenerator::new(key_manager);

        // Generate proof with specific key
        let proof = proof_gen
            .generate_proof_with_key(
                "POST",
                "https://api.example.com/token",
                None,
                Some(&key_pair),
            )
            .await?;

        // Validate proof
        let validation_result = proof_gen
            .validate_proof(&proof, "POST", "https://api.example.com/token", None)
            .await?;

        assert!(validation_result.valid);
        assert_eq!(validation_result.key_algorithm, algorithm);

        println!("✅ Algorithm {algorithm} works correctly");
    }

    Ok(())
}

/// Test error severity classification
#[test]
fn test_error_severity() {
    use turbomcp_dpop::{errors::ErrorSeverity, DpopError};

    let replay_error = DpopError::ReplayAttackDetected {
        nonce: "test".to_string(),
    };
    assert_eq!(replay_error.severity(), ErrorSeverity::Critical);

    let clock_error = DpopError::ClockSkewTooLarge {
        skew_seconds: 400,
        max_skew_seconds: 300,
    };
    assert_eq!(clock_error.severity(), ErrorSeverity::Medium);

    let crypto_error = DpopError::CryptographicError {
        reason: "test".to_string(),
    };
    assert_eq!(crypto_error.severity(), ErrorSeverity::High);
}

// Note: MCP error integration is handled at the application level
// where the main turbomcp crate provides conversion from DpopError to McpError

/// Benchmark DPoP proof generation performance
#[tokio::test]
async fn test_dpop_performance() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let proof_gen = DpopProofGenerator::new(key_manager);

    let start = SystemTime::now();
    let iterations = 10;

    for i in 0..iterations {
        let proof = proof_gen
            .generate_proof("POST", &format!("https://api.example.com/test{i}"), None)
            .await?;

        let _validation = proof_gen
            .validate_proof(
                &proof,
                "POST",
                &format!("https://api.example.com/test{i}"),
                None,
            )
            .await?;
    }

    let duration = start.elapsed().unwrap();
    let avg_ms = duration.as_millis() as f64 / iterations as f64;

    println!("✅ Average DPoP proof generation + validation: {avg_ms:.2}ms");

    // Performance threshold: should be under 50ms per operation in debug mode
    assert!(
        avg_ms < 50.0,
        "DPoP operations too slow: {avg_ms:.2}ms > 50ms"
    );

    Ok(())
}
