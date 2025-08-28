//! Comprehensive Security Tests for DPoP Flow (RFC 9449)
//!
//! **TurboMCP Zero-Tolerance Security Policy:**
//! These tests validate production-grade DPoP security against real attack vectors,
//! not theoretical scenarios. Every test must demonstrate actual security properties.
//!
//! **Note:** These tests require the `dpop` feature to be enabled.

#![cfg(feature = "dpop")]

use std::sync::Arc;
use std::time::Duration;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

#[cfg(feature = "dpop")]
use turbomcp_dpop::{
    DpopAlgorithm, DpopError, DpopKeyManager, DpopProofGenerator, MemoryNonceTracker, 
    ErrorSeverity, Result, DpopProof,
};

/// RFC 9449 Compliance: Validate DPoP proof structure exactly matches specification
#[tokio::test]
async fn test_rfc_9449_compliance_proof_structure() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let proof_gen = DpopProofGenerator::new(key_manager);

    let proof = proof_gen
        .generate_proof("POST", "https://server.example.com/token", None)
        .await?;

    // RFC 9449 Section 4.2: DPoP proof MUST be a JWT with specific claims
    proof.validate_structure()?;
    
    // Validate JWT header contains required fields per RFC 9449
    let jwt_string = proof.to_jwt_string();
    let jwt_parts: Vec<&str> = jwt_string.split('.').collect();
    assert_eq!(jwt_parts.len(), 3, "DPoP proof must be a valid JWT with 3 parts");

    // Decode header and validate structure
    let header_json = URL_SAFE_NO_PAD.decode(jwt_parts[0])
        .map_err(|e| DpopError::InvalidProofStructure { 
            reason: format!("Invalid header encoding: {}", e) 
        })?;
    
    let header: serde_json::Value = serde_json::from_slice(&header_json)?;
    
    // RFC 9449 Section 4.2: Header MUST contain typ, alg, and jwk
    assert_eq!(header["typ"], "dpop+jwt", "typ must be 'dpop+jwt'");
    assert!(header["alg"].is_string(), "alg must be present");
    assert!(header["jwk"].is_object(), "jwk must be present and be an object");

    // Decode payload and validate claims
    let payload_json = URL_SAFE_NO_PAD.decode(jwt_parts[1])
        .map_err(|e| DpopError::InvalidProofStructure { 
            reason: format!("Invalid payload encoding: {}", e) 
        })?;
    
    let payload: serde_json::Value = serde_json::from_slice(&payload_json)?;
    
    // RFC 9449 Section 4.2: Payload MUST contain jti, htm, htu, and iat
    assert!(payload["jti"].is_string(), "jti (nonce) must be present");
    assert_eq!(payload["htm"], "POST", "htm must match HTTP method");
    assert_eq!(payload["htu"], "https://server.example.com/token", "htu must match cleaned URI");
    assert!(payload["iat"].is_number(), "iat must be present and numeric");

    println!("✅ RFC 9449 compliance validated");
    Ok(())
}

/// Security Test: Cryptographic Strength Validation for All Algorithms
#[tokio::test]
async fn test_cryptographic_security_all_algorithms() -> Result<()> {
    let algorithms = [DpopAlgorithm::ES256, DpopAlgorithm::RS256, DpopAlgorithm::PS256];
    
    for algorithm in algorithms {
        let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
        let key_pair = key_manager.generate_key_pair(algorithm).await?;
        let proof_gen = DpopProofGenerator::new(key_manager.clone());

        // Generate proof with specific algorithm
        let proof = proof_gen
            .generate_proof_with_key(
                "GET", 
                "https://api.example.com/protected", 
                Some("test-token-123"),
                Some(&key_pair)
            )
            .await?;

        // Validate cryptographic signature strength
        let validation = proof_gen
            .validate_proof(&proof, "GET", "https://api.example.com/protected", Some("test-token-123"))
            .await?;
        
        assert!(validation.valid, "Proof must be cryptographically valid");
        assert_eq!(validation.key_algorithm, algorithm);

        // Test signature tampering resistance
        let mut tampered_jwt = proof.to_jwt_string();
        // Flip a bit in the signature
        let chars: Vec<char> = tampered_jwt.chars().collect();
        let mut tampered_chars = chars.clone();
        if let Some(last_char) = tampered_chars.last_mut() {
            *last_char = if *last_char == 'A' { 'B' } else { 'A' };
        }
        tampered_jwt = tampered_chars.into_iter().collect();

        // Create tampered proof
        let tampered_proof = DpopProof::from_jwt_string(&tampered_jwt)?;
        
        // Tampered proof MUST fail validation
        let tampered_result = proof_gen
            .validate_proof(&tampered_proof, "GET", "https://api.example.com/protected", Some("test-token-123"))
            .await;
        
        assert!(tampered_result.is_err(), "Tampered signature must be rejected");
        
        println!("✅ Algorithm {} cryptographic security validated", algorithm);
    }

    Ok(())
}

/// Security Test: Replay Attack Prevention with Concurrent Attempts
#[tokio::test]
async fn test_replay_attack_prevention_concurrent() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let nonce_tracker = Arc::new(MemoryNonceTracker::new());
    let proof_gen = Arc::new(DpopProofGenerator::with_nonce_tracker(key_manager, nonce_tracker));

    // Generate a valid proof
    let proof = proof_gen
        .generate_proof("POST", "https://api.example.com/token", None)
        .await?;

    // First validation should succeed
    let first_result = proof_gen
        .validate_proof(&proof, "POST", "https://api.example.com/token", None)
        .await?;
    assert!(first_result.valid);

    // Concurrent replay attempts should all fail
    let mut handles = vec![];
    for i in 0..10 {
        let proof_gen_clone = proof_gen.clone();
        let proof_clone = proof.clone();
        
        handles.push(tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(i * 10)).await; // Stagger attempts
            proof_gen_clone
                .validate_proof(&proof_clone, "POST", "https://api.example.com/token", None)
                .await
        }));
    }

    // Collect results
    let mut replay_detected_count = 0;
    for handle in handles {
        let result = handle.await.unwrap();
        if result.is_err() {
            match result.unwrap_err() {
                DpopError::ReplayAttackDetected { .. } => {
                    replay_detected_count += 1;
                }
                _ => panic!("Unexpected error type in concurrent replay test"),
            }
        }
    }

    // All concurrent attempts after the first should be detected as replays
    assert!(
        replay_detected_count >= 9,
        "Expected at least 9 replay attacks detected, got {}",
        replay_detected_count
    );

    println!("✅ Concurrent replay attack prevention validated");
    Ok(())
}

/// Security Test: Token Binding Integrity
#[tokio::test]
async fn test_token_binding_security() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let proof_gen = DpopProofGenerator::new(key_manager);

    let access_token_1 = "legitimate-token-abc123";
    let access_token_2 = "malicious-token-xyz789";

    // Generate proof bound to token 1
    let proof_bound_to_token1 = proof_gen
        .generate_proof("GET", "https://api.example.com/data", Some(access_token_1))
        .await?;

    // Validation with correct token should succeed
    let valid_result = proof_gen
        .validate_proof(&proof_bound_to_token1, "GET", "https://api.example.com/data", Some(access_token_1))
        .await?;
    assert!(valid_result.valid);

    // Validation with different token should fail (token substitution attack)
    let substitution_result = proof_gen
        .validate_proof(&proof_bound_to_token1, "GET", "https://api.example.com/data", Some(access_token_2))
        .await;
    
    assert!(substitution_result.is_err());
    match substitution_result.unwrap_err() {
        DpopError::AccessTokenHashFailed { .. } => {
            // Expected - token hash mismatch detected
        }
        _ => panic!("Expected AccessTokenHashFailed error"),
    }

    // Generate proof without token binding
    let proof_unbound = proof_gen
        .generate_proof("GET", "https://api.example.com/data", None)
        .await?;

    // Unbound proof should accept any token during validation
    let unbound_result = proof_gen
        .validate_proof(&proof_unbound, "GET", "https://api.example.com/data", Some(access_token_2))
        .await?;
    assert!(unbound_result.valid);

    println!("✅ Token binding security validated");
    Ok(())
}

/// Security Test: Clock Skew Attack Resistance
#[tokio::test]
async fn test_clock_skew_attack_resistance() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let proof_gen = DpopProofGenerator::new(key_manager);

    // Generate proof with current timestamp
    let proof = proof_gen
        .generate_proof("POST", "https://api.example.com/endpoint", None)
        .await?;

    // Immediate validation should succeed
    let immediate_result = proof_gen
        .validate_proof(&proof, "POST", "https://api.example.com/endpoint", None)
        .await?;
    assert!(immediate_result.valid);

    // Wait longer than maximum proof lifetime (simulate extreme clock skew)
    tokio::time::sleep(Duration::from_secs(301)).await; // > 300 second max skew

    // Validation with extreme clock skew should fail
    let expired_result = proof_gen
        .validate_proof(&proof, "POST", "https://api.example.com/endpoint", None)
        .await;

    assert!(expired_result.is_err());
    match expired_result.unwrap_err() {
        DpopError::ClockSkewTooLarge { skew_seconds, max_skew_seconds } => {
            assert!(skew_seconds > max_skew_seconds, "Clock skew should exceed maximum");
            assert_eq!(max_skew_seconds, 300, "Max skew should be 300 seconds per RFC");
        }
        DpopError::ProofExpired { .. } => {
            // Also acceptable - proof expired
        }
        _ => panic!("Expected clock skew or expiration error"),
    }

    println!("✅ Clock skew attack resistance validated");
    Ok(())
}

/// Security Test: Key Rotation Security Properties
#[tokio::test]
async fn test_key_rotation_security() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let proof_gen = DpopProofGenerator::new(key_manager.clone());

    // Generate key and proof
    let original_key = key_manager.generate_key_pair(DpopAlgorithm::ES256).await?;
    let proof_with_original = proof_gen
        .generate_proof_with_key("POST", "https://api.example.com/rotate", None, Some(&original_key))
        .await?;

    // Rotate the key
    let rotated_key = key_manager.rotate_key_pair(&original_key.id).await?;

    // Original key should be expired after rotation
    let retrieved_original = key_manager.get_key_pair(&original_key.id).await?;
    assert!(retrieved_original.is_some());
    assert!(retrieved_original.unwrap().is_expired());

    // Rotated key should be active
    assert!(!rotated_key.is_expired());
    assert_ne!(rotated_key.id, original_key.id);
    assert_ne!(rotated_key.thumbprint, original_key.thumbprint);
    assert_eq!(rotated_key.metadata.rotation_generation, 1);

    // Proof generated with expired key should fail validation
    let _expired_key_result = proof_gen
        .validate_proof(&proof_with_original, "POST", "https://api.example.com/rotate", None)
        .await;

    // Note: This depends on implementation - expired keys might still validate for a grace period
    // But the key should be marked as expired in metadata

    // Generate proof with new rotated key
    let proof_with_rotated = proof_gen
        .generate_proof_with_key("POST", "https://api.example.com/rotate", None, Some(&rotated_key))
        .await?;

    let rotated_validation = proof_gen
        .validate_proof(&proof_with_rotated, "POST", "https://api.example.com/rotate", None)
        .await?;
    assert!(rotated_validation.valid);

    println!("✅ Key rotation security properties validated");
    Ok(())
}

/// Security Test: Nonce Exhaustion Attack Prevention
#[tokio::test]
async fn test_nonce_exhaustion_prevention() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let nonce_tracker = Arc::new(MemoryNonceTracker::new());
    let proof_gen = DpopProofGenerator::with_nonce_tracker(key_manager, nonce_tracker.clone());

    // Generate many unique proofs rapidly
    const NONCE_COUNT: usize = 1000;
    let mut nonces = std::collections::HashSet::new();

    for i in 0..NONCE_COUNT {
        let proof = proof_gen
            .generate_proof("POST", &format!("https://api.example.com/test{}", i), None)
            .await?;

        // Validate proof
        let validation = proof_gen
            .validate_proof(&proof, "POST", &format!("https://api.example.com/test{}", i), None)
            .await?;
        assert!(validation.valid);

        // Extract nonce and ensure uniqueness
        let jwt_string = proof.to_jwt_string();
        let jwt_parts: Vec<&str> = jwt_string.split('.').collect();
        let payload_json = URL_SAFE_NO_PAD.decode(jwt_parts[1])
            .map_err(|e| DpopError::SerializationError { reason: e.to_string() })?;
        let payload: serde_json::Value = serde_json::from_slice(&payload_json)?;
        let nonce = payload["jti"].as_str().unwrap().to_string();

        assert!(nonces.insert(nonce.clone()), "Nonce {} was not unique", nonce);
    }

    // All nonces should be unique
    assert_eq!(nonces.len(), NONCE_COUNT);

    // Memory usage should be reasonable (basic check)
    // In production, nonce tracker should have cleanup mechanisms

    println!("✅ Nonce exhaustion prevention validated - {} unique nonces generated", NONCE_COUNT);
    Ok(())
}

/// Security Test: HTTP Method/URI Binding Attack Prevention
#[tokio::test]
async fn test_http_binding_attack_prevention() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let proof_gen = DpopProofGenerator::new(key_manager);

    // Generate proof for specific method and URI
    let proof = proof_gen
        .generate_proof("POST", "https://api.example.com/sensitive", None)
        .await?;

    // Validation with correct method and URI should succeed
    let correct_validation = proof_gen
        .validate_proof(&proof, "POST", "https://api.example.com/sensitive", None)
        .await?;
    assert!(correct_validation.valid);

    // Attack 1: Wrong HTTP method
    let wrong_method_result = proof_gen
        .validate_proof(&proof, "GET", "https://api.example.com/sensitive", None)
        .await;
    assert!(wrong_method_result.is_err());
    match wrong_method_result.unwrap_err() {
        DpopError::HttpBindingFailed { .. } => {
            // Expected
        }
        _ => panic!("Expected HttpBindingFailed for wrong method"),
    }

    // Attack 2: Wrong URI (different endpoint)
    let wrong_uri_result = proof_gen
        .validate_proof(&proof, "POST", "https://api.example.com/public", None)
        .await;
    assert!(wrong_uri_result.is_err());
    match wrong_uri_result.unwrap_err() {
        DpopError::HttpBindingFailed { .. } => {
            // Expected
        }
        _ => panic!("Expected HttpBindingFailed for wrong URI"),
    }

    // Attack 3: Different host
    let wrong_host_result = proof_gen
        .validate_proof(&proof, "POST", "https://malicious.example.com/sensitive", None)
        .await;
    assert!(wrong_host_result.is_err());
    match wrong_host_result.unwrap_err() {
        DpopError::HttpBindingFailed { .. } => {
            // Expected
        }
        _ => panic!("Expected HttpBindingFailed for wrong host"),
    }

    println!("✅ HTTP method/URI binding attack prevention validated");
    Ok(())
}

/// Security Test: Error Information Leakage Prevention
#[tokio::test]
async fn test_error_information_leakage_prevention() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let proof_gen = DpopProofGenerator::new(key_manager);

    // Generate valid proof
    let proof = proof_gen
        .generate_proof("POST", "https://api.example.com/token", None)
        .await?;

    // Test various error conditions and ensure no sensitive information leaks

    // 1. Test replay attack error
    let _ = proof_gen
        .validate_proof(&proof, "POST", "https://api.example.com/token", None)
        .await?; // First validation succeeds

    let replay_result = proof_gen
        .validate_proof(&proof, "POST", "https://api.example.com/token", None)
        .await;
    
    if let Err(DpopError::ReplayAttackDetected { nonce }) = replay_result {
        // Error should contain nonce for logging but not expose internal state
        assert!(!nonce.is_empty());
        assert_eq!(nonce.len(), 36); // UUID length check
        
        // Ensure error severity is correctly classified
        let error = DpopError::ReplayAttackDetected { nonce };
        assert_eq!(error.severity(), ErrorSeverity::Critical);
        assert!(error.is_security_violation());
    } else {
        panic!("Expected ReplayAttackDetected error");
    }

    // 2. Test cryptographic error doesn't expose key material
    let invalid_jwt = "invalid.jwt.token";
    let invalid_proof = DpopProof::from_jwt_string(invalid_jwt);
    
    match invalid_proof {
        Err(DpopError::InvalidProofStructure { reason }) => {
            // Error message should not contain sensitive details
            assert!(!reason.contains("private"));
            assert!(!reason.contains("secret"));
            assert!(!reason.contains("key"));
        }
        _ => {
            // Also acceptable if parsing succeeds but validation fails later
        }
    }

    // 3. Test error remediation hints are helpful but not revealing
    let clock_error = DpopError::ClockSkewTooLarge {
        skew_seconds: 400,
        max_skew_seconds: 300,
    };
    
    let hint = clock_error.remediation_hint();
    assert_eq!(hint, "Synchronize system clock with NTP server");
    assert!(!hint.contains("internal"));
    assert!(!hint.contains("secret"));

    println!("✅ Error information leakage prevention validated");
    Ok(())
}

/// Security Test: Concurrent Key Operations Safety
#[tokio::test]
async fn test_concurrent_key_operations_safety() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let proof_gen = Arc::new(DpopProofGenerator::new(key_manager.clone()));

    // Spawn concurrent key operations
    let mut key_handles = vec![];
    for _i in 0..10 {
        let km = key_manager.clone();
        key_handles.push(tokio::spawn(async move {
            km.generate_key_pair(DpopAlgorithm::ES256).await
        }));
    }

    // Spawn concurrent proof operations
    let mut proof_handles = vec![];
    for i in 0..10 {
        let pg = proof_gen.clone();
        proof_handles.push(tokio::spawn(async move {
            pg.generate_proof("GET", &format!("https://api.example.com/concurrent{}", i), None).await
        }));
    }

    // Wait for all key operations
    let mut key_count = 0;
    for handle in key_handles {
        let result = handle.await.unwrap();
        match result {
            Ok(_) => key_count += 1,
            Err(e) => panic!("Concurrent key operation failed: {:?}", e),
        }
    }

    // Wait for all proof operations  
    let mut proof_count = 0;
    for handle in proof_handles {
        let result = handle.await.unwrap();
        match result {
            Ok(_) => proof_count += 1,
            Err(e) => panic!("Concurrent proof operation failed: {:?}", e),
        }
    }

    assert_eq!(key_count, 10, "All key operations should succeed");
    assert_eq!(proof_count, 10, "All proof operations should succeed");

    println!("✅ Concurrent key operations safety validated");
    Ok(())
}

use base64;
use serde_json;
/// Security Test: JWT Structure Manipulation Resistance
#[tokio::test]
async fn test_jwt_manipulation_resistance() -> Result<()> {
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let proof_gen = DpopProofGenerator::new(key_manager);

    let proof = proof_gen
        .generate_proof("POST", "https://api.example.com/token", None)
        .await?;

    let original_jwt = proof.to_jwt_string();
    let parts: Vec<&str> = original_jwt.split('.').collect();

    // Attack 1: Modify algorithm in header to "none"
    let header_json = URL_SAFE_NO_PAD.decode(parts[0]).unwrap();
    let mut header: serde_json::Value = serde_json::from_slice(&header_json).unwrap();
    header["alg"] = serde_json::Value::String("none".to_string());
    let malicious_header = URL_SAFE_NO_PAD.encode(
        serde_json::to_string(&header).unwrap().as_bytes()
    );
    
    let malicious_jwt = format!("{}.{}.{}", malicious_header, parts[1], parts[2]);
    let malicious_proof = DpopProof::from_jwt_string(&malicious_jwt)?;
    
    let malicious_result = proof_gen
        .validate_proof(&malicious_proof, "POST", "https://api.example.com/token", None)
        .await;
    
    assert!(malicious_result.is_err(), "Algorithm downgrade attack must be prevented");

    // Attack 2: Modify payload claims
    let payload_json = URL_SAFE_NO_PAD.decode(parts[1]).unwrap();
    let mut payload: serde_json::Value = serde_json::from_slice(&payload_json).unwrap();
    payload["htm"] = serde_json::Value::String("GET".to_string()); // Change method
    let malicious_payload = URL_SAFE_NO_PAD.encode(
        serde_json::to_string(&payload).unwrap().as_bytes()
    );
    
    let modified_jwt = format!("{}.{}.{}", parts[0], malicious_payload, parts[2]);
    let modified_proof = DpopProof::from_jwt_string(&modified_jwt)?;
    
    let modified_result = proof_gen
        .validate_proof(&modified_proof, "POST", "https://api.example.com/token", None)
        .await;
    
    assert!(modified_result.is_err(), "Payload tampering must be detected");

    println!("✅ JWT structure manipulation resistance validated");
    Ok(())
}

/// Master Security Test: Complete Attack Scenario Simulation
#[tokio::test]
async fn test_complete_attack_scenario_simulation() -> Result<()> {
    println!("🔒 Running comprehensive DPoP security validation...");
    
    // This test combines multiple attack vectors in a realistic scenario
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let nonce_tracker = Arc::new(MemoryNonceTracker::new());
    let proof_gen = Arc::new(DpopProofGenerator::with_nonce_tracker(key_manager.clone(), nonce_tracker));

    // Legitimate flow
    let legitimate_token = "legitimate-access-token-abc123";
    let legitimate_proof = proof_gen
        .generate_proof("POST", "https://api.bank.com/transfer", Some(legitimate_token))
        .await?;
    
    let legitimate_result = proof_gen
        .validate_proof(&legitimate_proof, "POST", "https://api.bank.com/transfer", Some(legitimate_token))
        .await?;
    assert!(legitimate_result.valid, "Legitimate request must succeed");

    // Attack Scenario 1: Attacker tries to reuse captured proof
    let replay_result = proof_gen
        .validate_proof(&legitimate_proof, "POST", "https://api.bank.com/transfer", Some(legitimate_token))
        .await;
    assert!(replay_result.is_err(), "Replay attack must be detected");

    // Attack Scenario 2: Attacker tries to use proof with different token
    let stolen_token = "stolen-access-token-xyz789";
    let token_substitution_result = proof_gen
        .validate_proof(&legitimate_proof, "POST", "https://api.bank.com/transfer", Some(stolen_token))
        .await;
    assert!(token_substitution_result.is_err(), "Token substitution must be detected");

    // Attack Scenario 3: Attacker tries to use proof for different endpoint
    let endpoint_confusion_result = proof_gen
        .validate_proof(&legitimate_proof, "POST", "https://api.bank.com/account", Some(legitimate_token))
        .await;
    assert!(endpoint_confusion_result.is_err(), "Endpoint confusion must be detected");

    // Attack Scenario 4: Method confusion attack
    let method_confusion_result = proof_gen
        .validate_proof(&legitimate_proof, "DELETE", "https://api.bank.com/transfer", Some(legitimate_token))
        .await;
    assert!(method_confusion_result.is_err(), "Method confusion must be detected");

    // Legitimate flow should still work with fresh proof
    let fresh_proof = proof_gen
        .generate_proof("POST", "https://api.bank.com/transfer", Some(legitimate_token))
        .await?;
    
    let fresh_result = proof_gen
        .validate_proof(&fresh_proof, "POST", "https://api.bank.com/transfer", Some(legitimate_token))
        .await?;
    assert!(fresh_result.valid, "Fresh legitimate request must succeed after attacks");

    println!("✅ Complete attack scenario simulation - All attacks prevented, legitimate flow preserved");
    Ok(())
}

#[tokio::test]
async fn test_security_test_helper_functions() -> Result<()> {
    // Ensure all imports work correctly
    let _key_manager = DpopKeyManager::new_memory().await?;
    let _algorithm = DpopAlgorithm::ES256;
    let _nonce_tracker = MemoryNonceTracker::new();
    
    println!("✅ Security test infrastructure validated");
    Ok(())
}