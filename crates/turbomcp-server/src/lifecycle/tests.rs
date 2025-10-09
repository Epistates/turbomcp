//! Comprehensive tests for the server lifecycle module

use crate::lifecycle::{HealthCheck, HealthStatus, ServerLifecycle, ServerState};
use std::sync::Arc;
use tokio::time::{Duration, Instant};

#[tokio::test]
async fn test_server_lifecycle_creation() {
    let lifecycle = ServerLifecycle::new();

    // Should start in Starting state
    assert_eq!(lifecycle.state().await, ServerState::Starting);

    // Should be healthy by default
    let health = lifecycle.health().await;
    assert!(health.healthy);
    assert!(health.details.is_empty());

    // Should have a valid timestamp
    let now = Instant::now();
    assert!(health.timestamp <= now);
}

#[tokio::test]
async fn test_server_lifecycle_default() {
    let lifecycle = ServerLifecycle::default();

    // Should start in Starting state
    assert_eq!(lifecycle.state().await, ServerState::Starting);

    // Should be healthy by default
    let health = lifecycle.health().await;
    assert!(health.healthy);
    assert!(health.details.is_empty());
}

#[tokio::test]
async fn test_server_state_transitions() {
    let lifecycle = ServerLifecycle::new();

    // Test all state transitions
    assert_eq!(lifecycle.state().await, ServerState::Starting);

    lifecycle.set_state(ServerState::Running).await;
    assert_eq!(lifecycle.state().await, ServerState::Running);

    lifecycle.set_state(ServerState::ShuttingDown).await;
    assert_eq!(lifecycle.state().await, ServerState::ShuttingDown);

    lifecycle.set_state(ServerState::Stopped).await;
    assert_eq!(lifecycle.state().await, ServerState::Stopped);

    // Test setting back to Starting
    lifecycle.set_state(ServerState::Starting).await;
    assert_eq!(lifecycle.state().await, ServerState::Starting);
}

#[tokio::test]
async fn test_server_start() {
    let lifecycle = ServerLifecycle::new();

    assert_eq!(lifecycle.state().await, ServerState::Starting);

    lifecycle.start().await;

    assert_eq!(lifecycle.state().await, ServerState::Running);
}

#[tokio::test]
async fn test_server_shutdown() {
    let lifecycle = ServerLifecycle::new();

    // Set to running first
    lifecycle.set_state(ServerState::Running).await;
    assert_eq!(lifecycle.state().await, ServerState::Running);

    // Test shutdown
    lifecycle.shutdown().await;

    assert_eq!(lifecycle.state().await, ServerState::ShuttingDown);
}

#[tokio::test]
async fn test_shutdown_signal() {
    let lifecycle = ServerLifecycle::new();

    // Subscribe to shutdown signal
    let mut signal = lifecycle.shutdown_signal();

    // Signal should not be received yet
    assert!(signal.try_recv().is_err());

    // Initiate shutdown
    lifecycle.shutdown().await;

    // Signal should now be received
    assert!(signal.try_recv().is_ok());
}

#[tokio::test]
async fn test_multiple_shutdown_subscribers() {
    let lifecycle = ServerLifecycle::new();

    // Multiple subscribers
    let mut signal1 = lifecycle.shutdown_signal();
    let mut signal2 = lifecycle.shutdown_signal();
    let mut signal3 = lifecycle.shutdown_signal();

    // No signals yet
    assert!(signal1.try_recv().is_err());
    assert!(signal2.try_recv().is_err());
    assert!(signal3.try_recv().is_err());

    // Initiate shutdown
    lifecycle.shutdown().await;

    // All subscribers should receive the signal
    assert!(signal1.try_recv().is_ok());
    assert!(signal2.try_recv().is_ok());
    assert!(signal3.try_recv().is_ok());
}

#[tokio::test]
async fn test_health_status_creation() {
    let healthy = HealthStatus::healthy();
    assert!(healthy.healthy);
    assert!(healthy.details.is_empty());

    let unhealthy = HealthStatus::unhealthy();
    assert!(!unhealthy.healthy);
    assert!(unhealthy.details.is_empty());
}

#[tokio::test]
async fn test_health_check_creation() {
    let healthy_check = HealthCheck::healthy("database");
    assert_eq!(healthy_check.name, "database");
    assert!(healthy_check.healthy);
    assert!(healthy_check.message.is_none());

    let unhealthy_check = HealthCheck::unhealthy("redis", "Connection timeout");
    assert_eq!(unhealthy_check.name, "redis");
    assert!(!unhealthy_check.healthy);
    assert_eq!(
        unhealthy_check.message.as_ref().unwrap(),
        "Connection timeout"
    );
}

#[tokio::test]
async fn test_update_health_status() {
    let lifecycle = ServerLifecycle::new();

    // Initially healthy
    let health = lifecycle.health().await;
    assert!(health.healthy);
    assert!(health.details.is_empty());

    // Create health checks
    let check1 = HealthCheck::healthy("database");
    let check2 = HealthCheck::healthy("cache");
    let checks = vec![check1, check2];

    // Update health
    lifecycle.update_health(true, checks).await;

    let health = lifecycle.health().await;
    assert!(health.healthy);
    assert_eq!(health.details.len(), 2);
    assert_eq!(health.details[0].name, "database");
    assert_eq!(health.details[1].name, "cache");
}

#[tokio::test]
async fn test_update_health_status_unhealthy() {
    let lifecycle = ServerLifecycle::new();

    // Create mixed health checks
    let check1 = HealthCheck::healthy("database");
    let check2 = HealthCheck::unhealthy("cache", "Connection lost");
    let checks = vec![check1, check2];

    // Update health as unhealthy
    lifecycle.update_health(false, checks).await;

    let health = lifecycle.health().await;
    assert!(!health.healthy);
    assert_eq!(health.details.len(), 2);
    assert!(health.details[0].healthy);
    assert!(!health.details[1].healthy);
    assert_eq!(
        health.details[1].message.as_ref().unwrap(),
        "Connection lost"
    );
}

#[tokio::test]
async fn test_add_health_check() {
    let lifecycle = ServerLifecycle::new();

    // Add healthy check
    let check1 = HealthCheck::healthy("database");
    lifecycle.add_health_check(check1).await;

    let health = lifecycle.health().await;
    assert!(health.healthy);
    assert_eq!(health.details.len(), 1);
    assert_eq!(health.details[0].name, "database");

    // Add another healthy check
    let check2 = HealthCheck::healthy("cache");
    lifecycle.add_health_check(check2).await;

    let health = lifecycle.health().await;
    assert!(health.healthy);
    assert_eq!(health.details.len(), 2);

    // Add unhealthy check - should make overall status unhealthy
    let check3 = HealthCheck::unhealthy("api", "Timeout");
    lifecycle.add_health_check(check3).await;

    let health = lifecycle.health().await;
    assert!(!health.healthy); // Should be false because one check is unhealthy
    assert_eq!(health.details.len(), 3);
}

#[tokio::test]
async fn test_add_health_check_updates_timestamp() {
    let lifecycle = ServerLifecycle::new();

    let initial_health = lifecycle.health().await;
    let initial_timestamp = initial_health.timestamp;

    // Small delay to ensure timestamp difference
    tokio::time::sleep(Duration::from_millis(10)).await;

    let check = HealthCheck::healthy("test");
    lifecycle.add_health_check(check).await;

    let updated_health = lifecycle.health().await;
    assert!(updated_health.timestamp > initial_timestamp);
}

#[tokio::test]
async fn test_health_check_timestamps() {
    let check1 = HealthCheck::healthy("test1");
    let timestamp1 = check1.timestamp;

    // Small delay
    tokio::time::sleep(Duration::from_millis(10)).await;

    let check2 = HealthCheck::unhealthy("test2", "error");
    let timestamp2 = check2.timestamp;

    assert!(timestamp2 > timestamp1);
}

#[tokio::test]
async fn test_server_state_equality() {
    assert_eq!(ServerState::Starting, ServerState::Starting);
    assert_eq!(ServerState::Running, ServerState::Running);
    assert_eq!(ServerState::ShuttingDown, ServerState::ShuttingDown);
    assert_eq!(ServerState::Stopped, ServerState::Stopped);

    assert_ne!(ServerState::Starting, ServerState::Running);
    assert_ne!(ServerState::Running, ServerState::ShuttingDown);
    assert_ne!(ServerState::ShuttingDown, ServerState::Stopped);
}

#[tokio::test]
async fn test_server_state_debug() {
    let states = [
        ServerState::Starting,
        ServerState::Running,
        ServerState::ShuttingDown,
        ServerState::Stopped,
    ];

    for state in states {
        let debug_str = format!("{state:?}");
        assert!(!debug_str.is_empty());

        // Test that debug string contains expected content
        match state {
            ServerState::Starting => assert!(debug_str.contains("Starting")),
            ServerState::Running => assert!(debug_str.contains("Running")),
            ServerState::ShuttingDown => assert!(debug_str.contains("ShuttingDown")),
            ServerState::Stopped => assert!(debug_str.contains("Stopped")),
        }
    }
}

#[tokio::test]
async fn test_server_state_clone_copy() {
    let state = ServerState::Running;
    let cloned = state;
    let copied = state;

    assert_eq!(state, cloned);
    assert_eq!(state, copied);
}

#[tokio::test]
async fn test_health_status_clone() {
    let status = HealthStatus {
        healthy: true,
        timestamp: Instant::now(),
        details: vec![HealthCheck::healthy("test")],
    };

    let cloned = status.clone();

    assert_eq!(status.healthy, cloned.healthy);
    assert_eq!(status.timestamp, cloned.timestamp);
    assert_eq!(status.details.len(), cloned.details.len());
    assert_eq!(status.details[0].name, cloned.details[0].name);
}

#[tokio::test]
async fn test_health_check_clone() {
    let check = HealthCheck {
        name: "database".to_string(),
        healthy: true,
        message: Some("All good".to_string()),
        timestamp: Instant::now(),
    };

    let cloned = check.clone();

    assert_eq!(check.name, cloned.name);
    assert_eq!(check.healthy, cloned.healthy);
    assert_eq!(check.message, cloned.message);
    assert_eq!(check.timestamp, cloned.timestamp);
}

#[tokio::test]
async fn test_lifecycle_debug() {
    let lifecycle = ServerLifecycle::new();
    let debug_str = format!("{lifecycle:?}");

    assert!(!debug_str.is_empty());
    assert!(debug_str.contains("ServerLifecycle"));
}

#[tokio::test]
async fn test_health_status_debug() {
    let status = HealthStatus::healthy();
    let debug_str = format!("{status:?}");

    assert!(!debug_str.is_empty());
    assert!(debug_str.contains("HealthStatus"));
}

#[tokio::test]
async fn test_health_check_debug() {
    let check = HealthCheck::healthy("test");
    let debug_str = format!("{check:?}");

    assert!(!debug_str.is_empty());
    assert!(debug_str.contains("HealthCheck"));
    assert!(debug_str.contains("test"));
}

// Test concurrent access
#[tokio::test]
async fn test_concurrent_state_access() {
    let lifecycle = Arc::new(ServerLifecycle::new());

    let mut handles = Vec::new();

    // Spawn multiple tasks that access state concurrently
    for i in 0..10 {
        let lifecycle_clone = Arc::clone(&lifecycle);
        let handle = tokio::spawn(async move {
            if i % 2 == 0 {
                lifecycle_clone.set_state(ServerState::Running).await;
            } else {
                let _state = lifecycle_clone.state().await;
            }
        });
        handles.push(handle);
    }

    // Wait for all tasks to complete
    for handle in handles {
        handle.await.unwrap();
    }

    // State should be valid
    let final_state = lifecycle.state().await;
    assert!(matches!(
        final_state,
        ServerState::Starting | ServerState::Running
    ));
}

#[tokio::test]
async fn test_concurrent_health_access() {
    let lifecycle = Arc::new(ServerLifecycle::new());

    let mut handles = Vec::new();

    // Spawn multiple tasks that access health concurrently
    for i in 0..10 {
        let lifecycle_clone = Arc::clone(&lifecycle);
        let handle = tokio::spawn(async move {
            if i % 2 == 0 {
                let check = HealthCheck::healthy(format!("service_{i}"));
                lifecycle_clone.add_health_check(check).await;
            } else {
                let _health = lifecycle_clone.health().await;
            }
        });
        handles.push(handle);
    }

    // Wait for all tasks to complete
    for handle in handles {
        handle.await.unwrap();
    }

    // Health should still be valid
    let final_health = lifecycle.health().await;
    assert!(final_health.details.len() <= 5); // At most 5 checks were added (i % 2 == 0)
}

#[tokio::test]
async fn test_empty_health_checks_list() {
    let lifecycle = ServerLifecycle::new();

    // Update with empty checks list
    lifecycle.update_health(true, vec![]).await;

    let health = lifecycle.health().await;
    assert!(health.healthy);
    assert!(health.details.is_empty());

    // Update with false but empty list
    lifecycle.update_health(false, vec![]).await;

    let health = lifecycle.health().await;
    assert!(!health.healthy);
    assert!(health.details.is_empty());
}

#[tokio::test]
async fn test_health_check_with_different_string_types() {
    // Test with &str
    let check1 = HealthCheck::healthy("database");
    assert_eq!(check1.name, "database");

    // Test with String
    let check2 = HealthCheck::healthy(String::from("cache"));
    assert_eq!(check2.name, "cache");

    // Test unhealthy with different string types
    let check3 = HealthCheck::unhealthy("api", "error message");
    assert_eq!(check3.name, "api");
    assert_eq!(check3.message.unwrap(), "error message");

    let check4 = HealthCheck::unhealthy(String::from("redis"), String::from("timeout"));
    assert_eq!(check4.name, "redis");
    assert_eq!(check4.message.unwrap(), "timeout");
}

#[tokio::test]
async fn test_shutdown_signal_receiver_drop() {
    let lifecycle = ServerLifecycle::new();

    // Create a receiver and drop it
    {
        let _signal = lifecycle.shutdown_signal();
    } // signal is dropped here

    // Should still be able to create new receivers and shutdown
    let mut signal = lifecycle.shutdown_signal();
    lifecycle.shutdown().await;

    assert!(signal.try_recv().is_ok());
}

#[tokio::test]
async fn test_multiple_health_checks_same_name() {
    let lifecycle = ServerLifecycle::new();

    // Add multiple checks with the same name
    lifecycle
        .add_health_check(HealthCheck::healthy("database"))
        .await;
    lifecycle
        .add_health_check(HealthCheck::healthy("database"))
        .await;
    lifecycle
        .add_health_check(HealthCheck::unhealthy("database", "error"))
        .await;

    let health = lifecycle.health().await;
    assert!(!health.healthy); // Should be unhealthy due to one failing check
    assert_eq!(health.details.len(), 3);

    // All should have the same name
    for check in &health.details {
        assert_eq!(check.name, "database");
    }
}

// Integration test with realistic scenario
#[tokio::test]
async fn test_realistic_server_lifecycle_scenario() {
    let lifecycle = ServerLifecycle::new();

    // 1. Server starts
    assert_eq!(lifecycle.state().await, ServerState::Starting);
    lifecycle.start().await;
    assert_eq!(lifecycle.state().await, ServerState::Running);

    // 2. Add various health checks
    lifecycle
        .add_health_check(HealthCheck::healthy("database"))
        .await;
    lifecycle
        .add_health_check(HealthCheck::healthy("redis"))
        .await;
    lifecycle
        .add_health_check(HealthCheck::healthy("external_api"))
        .await;

    let health = lifecycle.health().await;
    assert!(health.healthy);
    assert_eq!(health.details.len(), 3);

    // 3. One service becomes unhealthy
    lifecycle
        .add_health_check(HealthCheck::unhealthy(
            "external_api",
            "503 Service Unavailable",
        ))
        .await;

    let health = lifecycle.health().await;
    assert!(!health.healthy);
    assert_eq!(health.details.len(), 4);

    // 4. Server shutdown is initiated
    let mut shutdown_signal = lifecycle.shutdown_signal();
    lifecycle.shutdown().await;

    assert_eq!(lifecycle.state().await, ServerState::ShuttingDown);
    assert!(shutdown_signal.try_recv().is_ok());

    // 5. Server stops
    lifecycle.set_state(ServerState::Stopped).await;
    assert_eq!(lifecycle.state().await, ServerState::Stopped);
}
