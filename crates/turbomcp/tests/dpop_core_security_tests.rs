//! Core DPoP Security Tests - Essential Security Validations
//!
//! **Focused Security Testing:** This test suite validates the most critical
//! DPoP security properties without performance-intensive operations.

#![cfg(feature = "dpop")]

use std::sync::Arc;
use turbomcp_dpop::{
    DpopAlgorithm, DpopError, DpopKeyManager, DpopProofGenerator, ErrorSeverity,
    MemoryNonceTracker, Result,
};

/// Test 1: RFC 9449 Compliance - Core Structure Validation
#[tokio::test]
async fn test_rfc_compliance_core() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let proof_gen = DpopProofGenerator::new(key_manager);

    // Generate proof for OAuth token endpoint
    let proof = proof_gen
        .generate_proof("POST", "https://auth.example.com/token", None)
        .await?;

    // Validate structural compliance
    proof.validate_structure()?;

    // Validate JWT has 3 parts
    let jwt_string = proof.to_jwt_string();
    let parts: Vec<&str> = jwt_string.split('.').collect();
    assert_eq!(
        parts.len(),
        3,
        "DPoP proof must be a valid JWT with 3 parts"
    );

    println!("✅ RFC 9449 compliance validated");
    Ok(())
}

/// Test 2: Cryptographic Security - Signature Validation
#[tokio::test]
async fn test_cryptographic_security() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let proof_gen = DpopProofGenerator::new(key_manager);

    // Generate proof
    let proof = proof_gen
        .generate_proof("GET", "https://api.example.com/data", Some("token-123"))
        .await?;

    // Valid proof should pass validation
    let validation = proof_gen
        .validate_proof(
            &proof,
            "GET",
            "https://api.example.com/data",
            Some("token-123"),
        )
        .await?;
    assert!(
        validation.valid,
        "Valid proof must pass cryptographic validation"
    );

    println!("✅ Cryptographic security validated");
    Ok(())
}

/// Test 3: Replay Attack Prevention
#[tokio::test]
async fn test_replay_attack_prevention() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let nonce_tracker = Arc::new(MemoryNonceTracker::new());
    let proof_gen = DpopProofGenerator::with_nonce_tracker(key_manager, nonce_tracker);

    // Generate proof
    let proof = proof_gen
        .generate_proof("POST", "https://api.example.com/transfer", None)
        .await?;

    // First validation should succeed
    let first_result = proof_gen
        .validate_proof(&proof, "POST", "https://api.example.com/transfer", None)
        .await?;
    assert!(first_result.valid, "First proof validation must succeed");

    // Second validation should fail (replay attack)
    let replay_result = proof_gen
        .validate_proof(&proof, "POST", "https://api.example.com/transfer", None)
        .await;

    assert!(replay_result.is_err(), "Replay attack must be detected");
    match replay_result.unwrap_err() {
        DpopError::ReplayAttackDetected { .. } => {
            // Expected behavior
        }
        other => panic!("Expected ReplayAttackDetected, got: {:?}", other),
    }

    println!("✅ Replay attack prevention validated");
    Ok(())
}

/// Test 4: Token Binding Security
#[tokio::test]
async fn test_token_binding_security() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let proof_gen = DpopProofGenerator::new(key_manager);

    let legitimate_token = "legitimate-token-abc";
    let malicious_token = "malicious-token-xyz";

    // Generate proof bound to legitimate token
    let proof = proof_gen
        .generate_proof(
            "GET",
            "https://api.bank.com/balance",
            Some(legitimate_token),
        )
        .await?;

    // Validation with correct token should succeed
    let valid_result = proof_gen
        .validate_proof(
            &proof,
            "GET",
            "https://api.bank.com/balance",
            Some(legitimate_token),
        )
        .await?;
    assert!(valid_result.valid, "Legitimate token binding must work");

    // Validation with different token should fail
    let invalid_result = proof_gen
        .validate_proof(
            &proof,
            "GET",
            "https://api.bank.com/balance",
            Some(malicious_token),
        )
        .await;

    assert!(
        invalid_result.is_err(),
        "Token substitution attack must be prevented"
    );

    println!("✅ Token binding security validated");
    Ok(())
}

/// Test 5: HTTP Method/URI Binding
#[tokio::test]
async fn test_http_binding_security() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let proof_gen = DpopProofGenerator::new(key_manager);

    // Generate proof for specific method and URI
    let proof = proof_gen
        .generate_proof("DELETE", "https://api.example.com/user/123", None)
        .await?;

    // Correct method and URI should succeed
    let valid_result = proof_gen
        .validate_proof(&proof, "DELETE", "https://api.example.com/user/123", None)
        .await?;
    assert!(valid_result.valid, "Correct HTTP binding must work");

    // Wrong method should fail
    let wrong_method = proof_gen
        .validate_proof(&proof, "GET", "https://api.example.com/user/123", None)
        .await;
    assert!(wrong_method.is_err(), "Wrong HTTP method must be rejected");

    // Wrong URI should fail
    let wrong_uri = proof_gen
        .validate_proof(&proof, "DELETE", "https://api.example.com/user/456", None)
        .await;
    assert!(wrong_uri.is_err(), "Wrong URI must be rejected");

    println!("✅ HTTP method/URI binding security validated");
    Ok(())
}

/// Test 6: Error Security Classification
#[tokio::test]
async fn test_error_security_classification() -> Result<()> {
    // Test error severity classification
    let replay_error = DpopError::ReplayAttackDetected {
        nonce: "test-nonce-123".to_string(),
    };
    assert_eq!(replay_error.severity(), ErrorSeverity::Critical);
    assert!(replay_error.is_security_violation());

    let clock_error = DpopError::ClockSkewTooLarge {
        skew_seconds: 400,
        max_skew_seconds: 300,
    };
    assert_eq!(clock_error.severity(), ErrorSeverity::Medium);
    assert!(clock_error.is_clock_skew_error());

    let crypto_error = DpopError::CryptographicError {
        reason: "Invalid signature".to_string(),
    };
    assert_eq!(crypto_error.severity(), ErrorSeverity::High);
    assert!(crypto_error.is_cryptographic_error());

    println!("✅ Error security classification validated");
    Ok(())
}

/// Test 7: Key Rotation Security
#[tokio::test]
async fn test_key_rotation_security() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);

    // Generate and rotate key
    let original_key = key_manager.generate_key_pair(DpopAlgorithm::ES256).await?;
    let rotated_key = key_manager.rotate_key_pair(&original_key.id).await?;

    // Verify rotation properties
    assert_ne!(
        rotated_key.id, original_key.id,
        "Rotated key must have different ID"
    );
    assert_ne!(
        rotated_key.thumbprint, original_key.thumbprint,
        "Rotated key must have different thumbprint"
    );
    assert_eq!(
        rotated_key.algorithm, original_key.algorithm,
        "Algorithm must be preserved"
    );
    assert_eq!(
        rotated_key.metadata.rotation_generation, 1,
        "Generation counter must increment"
    );

    // Original key should be expired
    let retrieved_original = key_manager.get_key_pair(&original_key.id).await?;
    assert!(retrieved_original.is_some());
    assert!(
        retrieved_original.unwrap().is_expired(),
        "Original key must be expired after rotation"
    );

    println!("✅ Key rotation security validated");
    Ok(())
}

/// Test 8: Algorithm Security
#[tokio::test]
async fn test_algorithm_security() -> Result<()> {
    let algorithms = [
        DpopAlgorithm::ES256,
        DpopAlgorithm::RS256,
        DpopAlgorithm::PS256,
    ];

    for algorithm in algorithms {
        let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
        let key_pair = key_manager.generate_key_pair(algorithm).await?;
        let proof_gen = DpopProofGenerator::new(key_manager);

        // Generate proof with specific algorithm
        let proof = proof_gen
            .generate_proof_with_key(
                "POST",
                "https://api.example.com/test",
                None,
                Some(&key_pair),
            )
            .await?;

        // Validate proof works with the algorithm
        let validation = proof_gen
            .validate_proof(&proof, "POST", "https://api.example.com/test", None)
            .await?;

        assert!(
            validation.valid,
            "Algorithm {} must produce valid proofs",
            algorithm
        );
        assert_eq!(
            validation.key_algorithm, algorithm,
            "Algorithm must match in validation"
        );
    }

    println!("✅ Algorithm security validated for ES256, RS256, PS256");
    Ok(())
}

/// Test 9: Multi-Attack Scenario
#[tokio::test]
async fn test_multi_attack_scenario() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let nonce_tracker = Arc::new(MemoryNonceTracker::new());
    let proof_gen = DpopProofGenerator::with_nonce_tracker(key_manager, nonce_tracker);

    let token = "sensitive-banking-token";

    // Legitimate operation
    let legitimate_proof = proof_gen
        .generate_proof("POST", "https://bank.api.com/transfer", Some(token))
        .await?;

    let legitimate_result = proof_gen
        .validate_proof(
            &legitimate_proof,
            "POST",
            "https://bank.api.com/transfer",
            Some(token),
        )
        .await?;
    assert!(legitimate_result.valid, "Legitimate operation must succeed");

    // Attack 1: Replay the same proof
    let replay_attack = proof_gen
        .validate_proof(
            &legitimate_proof,
            "POST",
            "https://bank.api.com/transfer",
            Some(token),
        )
        .await;
    assert!(replay_attack.is_err(), "Replay attack must be prevented");

    // Attack 2: Use proof with different token
    let token_attack = proof_gen
        .validate_proof(
            &legitimate_proof,
            "POST",
            "https://bank.api.com/transfer",
            Some("stolen-token"),
        )
        .await;
    assert!(
        token_attack.is_err(),
        "Token substitution must be prevented"
    );

    // Attack 3: Use proof for different endpoint
    let endpoint_attack = proof_gen
        .validate_proof(
            &legitimate_proof,
            "POST",
            "https://bank.api.com/account",
            Some(token),
        )
        .await;
    assert!(
        endpoint_attack.is_err(),
        "Endpoint confusion must be prevented"
    );

    // Attack 4: Use proof with different method
    let method_attack = proof_gen
        .validate_proof(
            &legitimate_proof,
            "DELETE",
            "https://bank.api.com/transfer",
            Some(token),
        )
        .await;
    assert!(method_attack.is_err(), "Method confusion must be prevented");

    // Fresh legitimate operation should still work
    let fresh_proof = proof_gen
        .generate_proof("POST", "https://bank.api.com/transfer", Some(token))
        .await?;

    let fresh_result = proof_gen
        .validate_proof(
            &fresh_proof,
            "POST",
            "https://bank.api.com/transfer",
            Some(token),
        )
        .await?;
    assert!(
        fresh_result.valid,
        "Fresh legitimate operation must work after attacks"
    );

    println!("✅ Multi-attack scenario - All attacks prevented, legitimate flow preserved");
    Ok(())
}

/// Test 10: Performance Security (Non-blocking)
#[tokio::test]
async fn test_performance_security() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let proof_gen = DpopProofGenerator::new(key_manager);

    let start = std::time::Instant::now();

    // Generate and validate a reasonable number of proofs
    for i in 0..5 {
        let proof = proof_gen
            .generate_proof("GET", &format!("https://api.example.com/item/{}", i), None)
            .await?;

        let validation = proof_gen
            .validate_proof(
                &proof,
                "GET",
                &format!("https://api.example.com/item/{}", i),
                None,
            )
            .await?;

        assert!(validation.valid, "Proof {} must be valid", i);
    }

    let duration = start.elapsed();
    let avg_ms = duration.as_millis() as f64 / 5.0;

    // Performance check: should be reasonable (under 100ms per operation in debug mode)
    assert!(
        avg_ms < 100.0,
        "DPoP operations too slow: {:.2}ms > 100ms per operation",
        avg_ms
    );

    println!(
        "✅ Performance security validated - Average {:.2}ms per operation",
        avg_ms
    );
    Ok(())
}
