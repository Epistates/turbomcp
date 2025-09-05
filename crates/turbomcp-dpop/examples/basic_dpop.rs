//! Basic DPoP Usage Example
//!
//! This example demonstrates the fundamental DPoP operations:
//! - Key generation
//! - Proof creation
//! - Proof validation
//! - Integration with HTTP requests

use std::sync::Arc;

use turbomcp_dpop::{DpopAlgorithm, DpopKeyManager, DpopProofGenerator, Result};

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 TurboMCP DPoP Basic Example");
    println!("==============================\n");

    // Step 1: Create a key manager and generate a key pair
    println!("📋 Step 1: Generate DPoP key pair");
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let key_pair = key_manager.generate_key_pair(DpopAlgorithm::ES256).await?;

    println!("✅ Generated key pair:");
    println!("   Algorithm: {}", key_pair.algorithm);
    println!("   Thumbprint: {}", key_pair.thumbprint);
    println!("   Key ID: {}\n", key_pair.id);

    // Step 2: Create a DPoP proof generator
    println!("📋 Step 2: Create DPoP proof for OAuth token request");
    let proof_gen = DpopProofGenerator::new(key_manager.clone());

    let method = "POST";
    let uri = "https://auth.example.com/oauth/token";
    let access_token = None; // No access token for initial OAuth request

    let proof = proof_gen.generate_proof(method, uri, access_token).await?;

    println!("✅ Generated DPoP proof:");
    println!("   Method: {}", proof.payload.htm);
    println!("   URI: {}", proof.payload.htu);
    println!("   JTI (nonce): {}", proof.payload.jti);
    println!("   JWT: {}\n", proof.to_jwt_string());

    // Step 3: Simulate sending HTTP request with DPoP header
    println!("📋 Step 3: Simulate HTTP request with DPoP header");
    println!("HTTP Request would include:");
    println!("   POST {}", uri);
    println!("   DPoP: {}", proof.to_jwt_string());
    println!("   Content-Type: application/x-www-form-urlencoded\n");

    // Step 4: Validate the DPoP proof (server-side validation)
    println!("📋 Step 4: Validate DPoP proof (server-side)");
    let validation_result = proof_gen
        .validate_proof(&proof, method, uri, access_token)
        .await?;

    println!("✅ Proof validation result:");
    println!("   Valid: {}", validation_result.valid);
    println!("   Thumbprint: {}", validation_result.thumbprint);
    println!("   Algorithm: {}", validation_result.key_algorithm);
    println!("   Issued at: {:?}", validation_result.issued_at);
    println!("   Expires at: {:?}\n", validation_result.expires_at);

    // Step 5: Generate proof for protected resource access
    println!("📋 Step 5: Generate proof for protected resource access");
    let protected_method = "GET";
    let protected_uri = "https://api.example.com/user/profile";
    let access_token = "access_token_received_from_oauth";

    let protected_proof = proof_gen
        .generate_proof(protected_method, protected_uri, Some(access_token))
        .await?;

    println!("✅ Generated proof for protected resource:");
    println!("   Method: {}", protected_proof.payload.htm);
    println!("   URI: {}", protected_proof.payload.htu);
    println!(
        "   Has access token hash: {}",
        protected_proof.payload.ath.is_some()
    );
    println!("   JWT: {}\n", protected_proof.to_jwt_string());

    // Step 6: Show what the complete HTTP request would look like
    println!("📋 Step 6: Complete HTTP request with DPoP");
    println!("HTTP Request:");
    println!("   GET {}", protected_uri);
    println!("   Authorization: Bearer {}", access_token);
    println!("   DPoP: {}", protected_proof.to_jwt_string());
    println!("   Accept: application/json\n");

    // Step 7: Demonstrate replay attack prevention
    println!("📋 Step 7: Demonstrate replay attack prevention");

    // First validation should succeed
    let first_validation = proof_gen
        .validate_proof(
            &protected_proof,
            protected_method,
            protected_uri,
            Some(access_token),
        )
        .await?;
    println!("✅ First validation: {}", first_validation.valid);

    // Second validation should fail (replay attack detected)
    let replay_result = proof_gen
        .validate_proof(
            &protected_proof,
            protected_method,
            protected_uri,
            Some(access_token),
        )
        .await;

    match replay_result {
        Ok(_) => println!("❌ Replay attack not detected (this shouldn't happen!)"),
        Err(e) => println!("✅ Replay attack prevented: {}", e),
    }

    println!("\n🎉 DPoP example completed successfully!");
    println!("\nKey Benefits Demonstrated:");
    println!("  ✅ Token binding prevents stolen token usage");
    println!("  ✅ HTTP method/URI binding prevents cross-endpoint attacks");
    println!("  ✅ Replay attack prevention with nonce tracking");
    println!("  ✅ Access token hash binding for added security");

    Ok(())
}
