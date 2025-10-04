//! DPoP Automated Key Rotation Example
//!
//! This example demonstrates enterprise-grade automated key rotation for DPoP,
//! providing continuous security through background key lifecycle management.
//!
//! ## Key Features
//!
//! - **Background Service**: Tokio-based scheduler runs rotation checks at intervals
//! - **Production Policies**: Configurable rotation intervals and key lifetimes
//! - **Graceful Management**: Clean service startup/shutdown with cancellation
//! - **Comprehensive Monitoring**: Success/failure metrics, error tracking, alerting
//! - **Zero Downtime**: Seamless key transitions without service interruption
//!
//! ## Use Cases
//!
//! - **Long-running Services**: Applications that run for days/weeks/months
//! - **High Security Environments**: Regular key rotation for forward security
//! - **Distributed Systems**: Consistent key rotation across multiple instances
//! - **Compliance Requirements**: Automated security practices for auditing

#[cfg(feature = "dpop")]
use std::sync::Arc;
#[cfg(feature = "dpop")]
use std::time::Duration;

#[cfg(feature = "dpop")]
use turbomcp::auth::dpop::{
    AutoRotationService, DpopAlgorithm, DpopKeyManager, KeyRotationPolicy, RotationMetricsSnapshot,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "dpop")]
    {
        return dpop_example_main().await;
    }

    #[cfg(not(feature = "dpop"))]
    {
        println!("❌ DPoP feature not enabled");
        println!("   Add 'dpop' feature to Cargo.toml:");
        println!("   turbomcp = {{ version = \"1.1.0\", features = [\"dpop\"] }}");
        return Ok(());
    }
}

#[cfg(feature = "dpop")]
async fn dpop_example_main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for monitoring the rotation service
    tracing_subscriber::fmt()
        .with_env_filter("debug,turbomcp_dpop=trace")
        .init();

    println!("🔄 TurboMCP DPoP Automated Key Rotation Example");
    println!("==============================================\n");

    // Demonstrate development vs production policies
    demonstrate_rotation_policies().await?;

    println!("\n{}\n", "=".repeat(60));

    // Demonstrate the auto-rotation service
    demonstrate_auto_rotation_service().await?;

    Ok(())
}

/// Compare development vs production rotation policies
#[cfg(feature = "dpop")]
async fn demonstrate_rotation_policies() -> Result<(), Box<dyn std::error::Error>> {
    println!("📋 Key Rotation Policy Configurations");
    println!("------------------------------------\n");

    // Development policy - shorter intervals for testing
    let dev_policy = KeyRotationPolicy::development();
    println!("🧪 Development Policy:");
    println!(
        "   • Key lifetime: {} hours",
        dev_policy.key_lifetime.as_secs() / 3600
    );
    println!(
        "   • Auto-rotation: {}",
        if dev_policy.auto_rotate {
            "✅ Enabled"
        } else {
            "❌ Disabled"
        }
    );
    println!(
        "   • Check interval: {} minutes",
        dev_policy.rotation_check_interval.as_secs() / 60
    );

    // Production policy - longer intervals, auto-rotation enabled
    let prod_policy = KeyRotationPolicy::production();
    println!("\n🏭 Production Policy:");
    println!(
        "   • Key lifetime: {} days",
        prod_policy.key_lifetime.as_secs() / (24 * 3600)
    );
    println!(
        "   • Auto-rotation: {}",
        if prod_policy.auto_rotate {
            "✅ Enabled"
        } else {
            "❌ Disabled"
        }
    );
    println!(
        "   • Check interval: {} minutes",
        prod_policy.rotation_check_interval.as_secs() / 60
    );

    // Custom policy for this example - very short intervals for demonstration
    let demo_policy = KeyRotationPolicy {
        key_lifetime: Duration::from_secs(10), // 10 seconds for demo
        auto_rotate: true,
        rotation_check_interval: Duration::from_secs(3), // Check every 3 seconds
    };
    println!("\n🎯 Demo Policy (for this example):");
    println!(
        "   • Key lifetime: {} seconds",
        demo_policy.key_lifetime.as_secs()
    );
    println!(
        "   • Auto-rotation: {}",
        if demo_policy.auto_rotate {
            "✅ Enabled"
        } else {
            "❌ Disabled"
        }
    );
    println!(
        "   • Check interval: {} seconds",
        demo_policy.rotation_check_interval.as_secs()
    );

    Ok(())
}

/// Demonstrate the automated rotation service in action
#[cfg(feature = "dpop")]
async fn demonstrate_auto_rotation_service() -> Result<(), Box<dyn std::error::Error>> {
    println!("🤖 Automated Key Rotation Service");
    println!("--------------------------------\n");

    // Create a key manager with fast rotation for demonstration
    let demo_policy = KeyRotationPolicy {
        key_lifetime: Duration::from_secs(5), // Very short for demo
        auto_rotate: true,
        rotation_check_interval: Duration::from_secs(2), // Check every 2 seconds
    };

    let key_manager = Arc::new(
        DpopKeyManager::new(
            Arc::new(turbomcp::auth::dpop::MemoryKeyStorage::new()),
            demo_policy.clone(),
        )
        .await?,
    );

    // Generate initial keys that will need rotation
    println!("🔑 Generating initial keys...");
    let key1 = key_manager.generate_key_pair(DpopAlgorithm::ES256).await?;
    let key2 = key_manager.generate_key_pair(DpopAlgorithm::RS256).await?;

    println!(
        "   ✅ Generated ES256 key: {} (generation: {})",
        &key1.id[..8],
        key1.metadata.rotation_generation
    );
    println!(
        "   ✅ Generated RS256 key: {} (generation: {})",
        &key2.id[..8],
        key2.metadata.rotation_generation
    );

    // Create and start the auto-rotation service
    println!("\n🚀 Starting auto-rotation service...");
    let mut rotation_service = AutoRotationService::new(key_manager.clone());
    rotation_service.start().await?;

    println!("   ✅ Auto-rotation service started");
    println!(
        "   ⏰ Keys will expire in {} seconds",
        demo_policy.key_lifetime.as_secs()
    );
    println!(
        "   🔍 Rotation checks every {} seconds",
        demo_policy.rotation_check_interval.as_secs()
    );

    // Monitor the service for rotations
    println!("\n📊 Monitoring rotation events...");
    let mut last_metrics = rotation_service.get_metrics().await;

    for i in 1..=6 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let metrics = rotation_service.get_metrics().await;

        println!("\n⏱️  Check {} ({}s elapsed):", i, i * 2);
        print_metrics_update(&last_metrics, &metrics);

        // Trigger manual rotation check if no automatic rotations yet
        if i == 3 && metrics.successful_rotations == 0 {
            println!("   🔄 Triggering manual rotation check...");
            rotation_service.trigger_rotation_check();
        }

        last_metrics = metrics;
    }

    // Show final service state
    println!("\n📈 Final Service State:");
    let final_metrics = rotation_service.get_metrics().await;
    print_detailed_metrics(&final_metrics);

    // Gracefully stop the service
    println!("\n🛑 Stopping auto-rotation service...");
    rotation_service.stop().await?;
    println!("   ✅ Auto-rotation service stopped gracefully");

    // Demonstrate service integration patterns
    demonstrate_service_patterns().await?;

    Ok(())
}

/// Print changes between two metrics snapshots
#[cfg(feature = "dpop")]
fn print_metrics_update(old: &RotationMetricsSnapshot, new: &RotationMetricsSnapshot) {
    if new.successful_rotations > old.successful_rotations {
        let diff = new.successful_rotations - old.successful_rotations;
        println!("   🎉 {} successful rotation(s)!", diff);
    }

    if new.failed_rotations > old.failed_rotations {
        let diff = new.failed_rotations - old.failed_rotations;
        println!("   ❌ {} failed rotation(s)", diff);
        if let Some((_time, error)) = &new.last_error {
            println!("      Error: {}", error);
        }
    }

    if new.successful_rotations == old.successful_rotations
        && new.failed_rotations == old.failed_rotations
    {
        println!("   💤 No rotations (keys not yet expired)");
    }

    println!("   📊 Tracked keys: {}", new.tracked_keys);
}

/// Print detailed metrics information
#[cfg(feature = "dpop")]
fn print_detailed_metrics(metrics: &RotationMetricsSnapshot) {
    println!(
        "   📊 Total successful rotations: {}",
        metrics.successful_rotations
    );
    println!("   ❌ Total failed rotations: {}", metrics.failed_rotations);
    println!("   🔑 Currently tracked keys: {}", metrics.tracked_keys);

    if let Some(last_rotation) = metrics.last_rotation_time {
        let elapsed = std::time::SystemTime::now()
            .duration_since(last_rotation)
            .unwrap_or(Duration::ZERO);
        println!(
            "   🕐 Last successful rotation: {:.1}s ago",
            elapsed.as_secs_f64()
        );
    }

    if let Some((error_time, error_msg)) = &metrics.last_error {
        let elapsed = std::time::SystemTime::now()
            .duration_since(*error_time)
            .unwrap_or(Duration::ZERO);
        println!(
            "   💥 Last error ({:.1}s ago): {}",
            elapsed.as_secs_f64(),
            error_msg
        );
    }
}

/// Demonstrate common service integration patterns
#[cfg(feature = "dpop")]
async fn demonstrate_service_patterns() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n🏗️  Service Integration Patterns");
    println!("------------------------------\n");

    println!("💡 Common Integration Patterns:");

    println!("\n1️⃣  **Production Service Lifecycle**");
    println!("```rust");
    println!("// Application startup");
    println!(
        "let key_manager = Arc::new(DpopKeyManager::new(storage, KeyRotationPolicy::production()).await?);"
    );
    println!("let mut rotation_service = AutoRotationService::new(key_manager.clone());");
    println!("rotation_service.start().await?;");
    println!("");
    println!("// Application shutdown (graceful)");
    println!("rotation_service.stop().await?;");
    println!("```");

    println!("\n2️⃣  **Health Monitoring & Alerting**");
    println!("```rust");
    println!("// Periodic health checks");
    println!("let metrics = rotation_service.get_metrics().await;");
    println!("if metrics.failed_rotations > threshold {{");
    println!("    alert_system.send_alert(\"DPoP key rotation failures\").await?;");
    println!("}}");
    println!("```");

    println!("\n3️⃣  **Manual Rotation Triggers**");
    println!("```rust");
    println!("// On security events or admin request");
    println!("rotation_service.trigger_rotation_check();");
    println!("// Force rotation of specific keys");
    println!("key_manager.rotate_key_pair(&key_id).await?;");
    println!("```");

    println!("\n🔧 Configuration Best Practices:");
    println!("   • Development: 24-hour key lifetime, hourly checks");
    println!("   • Staging: 3-day key lifetime, hourly checks");
    println!("   • Production: 7-day key lifetime, hourly checks");
    println!("   • High Security: 1-day key lifetime, 30-minute checks");

    println!("\n📊 Monitoring Recommendations:");
    println!("   • Track successful rotation rate");
    println!("   • Alert on consecutive rotation failures");
    println!("   • Monitor key age distribution");
    println!("   • Log rotation events for audit trails");

    println!("\n🛡️  Security Benefits:");
    println!("   • Forward secrecy through key rotation");
    println!("   • Reduced blast radius of key compromise");
    println!("   • Compliance with security best practices");
    println!("   • Automated security hygiene");

    Ok(())
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::time::Duration;
    use turbomcp::auth::dpop::{AutoRotationService, DpopKeyManager, KeyRotationPolicy};

    #[tokio::test]
    async fn test_auto_rotation_lifecycle() {
        let policy = KeyRotationPolicy {
            key_lifetime: Duration::from_secs(1), // Very short for test
            auto_rotate: true,
            rotation_check_interval: Duration::from_millis(500),
        };

        let key_manager = Arc::new(
            DpopKeyManager::new(Arc::new(turbomcp_dpop::MemoryKeyStorage::new()), policy)
                .await
                .unwrap(),
        );

        // Generate a key that will expire quickly
        let _initial_key = key_manager
            .generate_key_pair(DpopAlgorithm::ES256)
            .await
            .unwrap();

        // Start rotation service
        let mut service = AutoRotationService::new(key_manager);
        service.start().await.unwrap();

        // Wait for rotation to occur
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Check metrics
        let metrics = service.get_metrics().await;
        assert!(metrics.tracked_keys > 0, "Should track keys");

        // Stop service
        service.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_rotation_metrics() {
        let key_manager = Arc::new(
            DpopKeyManager::new(
                Arc::new(turbomcp::auth::dpop::MemoryKeyStorage::new()),
                KeyRotationPolicy::development(),
            )
            .await
            .unwrap(),
        );

        let service = AutoRotationService::new(key_manager);

        // Initial metrics should be zero
        let initial_metrics = service.get_metrics().await;
        assert_eq!(initial_metrics.successful_rotations, 0);
        assert_eq!(initial_metrics.failed_rotations, 0);
        assert!(initial_metrics.last_rotation_time.is_none());
        assert!(initial_metrics.last_error.is_none());
    }
}
