//! Redis-based DPoP nonce tracking example
//!
//! This example demonstrates how to use Redis for distributed DPoP nonce tracking,
//! providing replay protection across multiple server instances.
//!
//! ## Prerequisites
//!
//! 1. Enable the 'redis-storage' feature:
//!    ```toml
//!    [dependencies]
//!    turbomcp = { version = "1.1.0", features = ["redis-storage"] }
//!    ```
//!
//! 2. Start Redis server:
//!    ```bash
//!    docker run -p 6379:6379 redis:alpine
//!    ```
//!
//! ## Key Features
//!
//! - **Distributed Replay Protection**: Multiple servers share nonce state
//! - **Automatic Expiration**: Redis TTL handles cleanup
//! - **Production Ready**: Connection pooling, retry logic, error handling
//! - **Opt-in Architecture**: Falls back gracefully when feature disabled

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for observability
    tracing_subscriber::fmt()
        .with_env_filter("debug,redis=info")
        .init();

    println!("🔒 TurboMCP DPoP Redis Nonce Tracking Example");
    println!("===========================================\n");

    // Demonstrate feature-gated Redis usage
    #[cfg(feature = "redis-storage")]
    {
        demonstrate_redis_nonce_tracking().await?;
    }

    #[cfg(not(feature = "redis-storage"))]
    {
        println!("❌ Redis storage feature not enabled");
        println!("   Add 'redis-storage' feature to Cargo.toml:");
        println!("   turbomcp = {{ version = \"1.1.0\", features = [\"redis-storage\"] }}");
        println!();
        println!("   Then start Redis and run this example again.");
        println!();

        #[cfg(feature = "dpop")]
        {
            demonstrate_memory_fallback().await?;
        }

        #[cfg(not(feature = "dpop"))]
        {
            println!("💡 For memory-based fallback, enable the 'dpop' feature");
            println!("   turbomcp = {{ version = \"1.1.0\", features = [\"dpop\"] }}");
        }
    }

    Ok(())
}

/// Demonstrate Redis-based nonce tracking (when feature enabled)
#[cfg(feature = "redis-storage")]
async fn demonstrate_redis_nonce_tracking() -> Result<(), Box<dyn std::error::Error>> {
    use turbomcp_dpop::{DpopKeyManager, DpopProofGenerator, RedisNonceTracker};

    println!("🚀 Setting up Redis-based DPoP nonce tracking...");

    // Check if Redis is available
    let redis_url = "redis://127.0.0.1:6379";

    // Create Redis nonce tracker
    let redis_tracker = match RedisNonceTracker::new(redis_url).await {
        Ok(tracker) => {
            println!("✅ Connected to Redis successfully");
            tracker
        }
        Err(e) => {
            println!("❌ Failed to connect to Redis: {}", e);
            println!("   Make sure Redis is running: docker run -p 6379:6379 redis:alpine");
            return Ok(());
        }
    };

    // Create DPoP proof generator with Redis tracking
    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let proof_gen = DpopProofGenerator::with_nonce_tracker(key_manager, Arc::new(redis_tracker));

    println!("\n📝 Testing DPoP proof generation with Redis nonce tracking...");

    // Generate and validate first proof
    let uri = "https://api.turbomcp.org/oauth/token";
    let proof1 = proof_gen.generate_proof("POST", uri, None).await?;
    println!("   Generated proof 1: {}", &proof1.to_jwt_string()[..50]);

    // First validation should succeed
    let result1 = proof_gen.validate_proof(&proof1, "POST", uri, None).await?;
    println!(
        "   ✅ Proof 1 validation: {}",
        if result1.valid { "SUCCESS" } else { "FAILED" }
    );

    // Attempt replay attack - should fail
    match proof_gen.validate_proof(&proof1, "POST", uri, None).await {
        Ok(_) => println!("   ❌ Replay attack succeeded (should have failed!)"),
        Err(e) => println!(
            "   ✅ Replay attack blocked: {}",
            e.to_string().split(':').next().unwrap_or("Unknown")
        ),
    }

    // Generate second proof - should succeed
    let proof2 = proof_gen.generate_proof("POST", uri, None).await?;
    let result2 = proof_gen.validate_proof(&proof2, "POST", uri, None).await?;
    println!(
        "   ✅ Proof 2 validation: {}",
        if result2.valid { "SUCCESS" } else { "FAILED" }
    );

    println!("\n🔄 Testing distributed nonce tracking...");

    // Create second proof generator (simulating different server)
    let key_manager2 = Arc::new(DpopKeyManager::new_memory().await?);

    // Use same Redis instance (simulating shared state)
    let redis_tracker2 = RedisNonceTracker::new(redis_url)
        .await?
        .with_client_id("server-2".to_string());

    let proof_gen2 = DpopProofGenerator::with_nonce_tracker(key_manager2, Arc::new(redis_tracker2));

    // Generate proof on "server 2"
    let proof3 = proof_gen2.generate_proof("POST", uri, None).await?;
    let result3 = proof_gen2
        .validate_proof(&proof3, "POST", uri, None)
        .await?;
    println!(
        "   ✅ Server 2 proof validation: {}",
        if result3.valid { "SUCCESS" } else { "FAILED" }
    );

    println!("\n📊 Redis Storage Benefits:");
    println!("   • Distributed replay protection across multiple servers");
    println!("   • Automatic nonce expiration via Redis TTL");
    println!("   • Production-grade connection pooling and retry logic");
    println!("   • Persistent storage survives server restarts");

    Ok(())
}

/// Demonstrate memory-based fallback (when Redis feature disabled)
#[cfg(feature = "dpop")]
async fn demonstrate_memory_fallback() -> Result<(), Box<dyn std::error::Error>> {
    use turbomcp_dpop::{DpopKeyManager, DpopProofGenerator, MemoryNonceTracker};

    println!("🧠 Falling back to memory-based nonce tracking...");

    let key_manager = Arc::new(DpopKeyManager::new_memory().await?);
    let memory_tracker = Arc::new(MemoryNonceTracker::new());
    let proof_gen = DpopProofGenerator::with_nonce_tracker(key_manager, memory_tracker);

    println!("\n📝 Testing memory-based nonce tracking...");

    let uri = "https://api.turbomcp.org/oauth/token";

    // Generate and validate proof
    let proof = proof_gen.generate_proof("POST", uri, None).await?;
    let result = proof_gen.validate_proof(&proof, "POST", uri, None).await?;
    println!(
        "   ✅ Memory tracker validation: {}",
        if result.valid { "SUCCESS" } else { "FAILED" }
    );

    // Test replay protection
    match proof_gen.validate_proof(&proof, "POST", uri, None).await {
        Ok(_) => println!("   ❌ Replay attack succeeded (should have failed!)"),
        Err(e) => println!(
            "   ✅ Replay attack blocked: {}",
            e.to_string().split(':').next().unwrap_or("Unknown")
        ),
    }

    println!("\n⚠️  Memory Storage Limitations:");
    println!("   • Single server only - no distributed protection");
    println!("   • State lost on server restart");
    println!("   • Manual cleanup required for long-running processes");
    println!();
    println!("💡 Consider enabling 'redis-storage' feature for production use");

    Ok(())
}

#[cfg(feature = "redis-storage")]
mod redis_integration_tests {
    use super::*;
    use turbomcp_dpop::{DpopKeyManager, DpopProofGenerator, RedisNonceTracker};

    /// Test Redis nonce tracker with custom configuration
    #[tokio::test]
    #[ignore] // Requires Redis server
    async fn test_redis_custom_config() {
        let tracker = RedisNonceTracker::with_config(
            "redis://127.0.0.1:6379",
            Duration::from_secs(300), // 5 minutes
            "test-app".to_string(),
        )
        .await
        .expect("Redis connection failed");

        let key_manager = Arc::new(DpopKeyManager::new_memory().await.unwrap());
        let proof_gen = DpopProofGenerator::with_nonce_tracker(
            key_manager,
            Arc::new(tracker.with_client_id("test-client".to_string())),
        );

        // Test basic functionality
        let proof = proof_gen
            .generate_proof("POST", "https://test.example.com/token", None)
            .await
            .unwrap();

        let result = proof_gen
            .validate_proof(&proof, "POST", "https://test.example.com/token", None)
            .await
            .unwrap();

        assert!(result.valid);
    }
}
