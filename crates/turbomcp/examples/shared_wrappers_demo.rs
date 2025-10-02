//! Comprehensive demonstration of TurboMCP shared wrappers
//!
//! This example shows how to use all the shared wrapper types together:
//! - SharedTransport for transport layer sharing
//! - McpServer uses Clone (Axum/Tower pattern) for server instance sharing
//! - SharedElicitationCoordinator for elicitation sharing
//! - Generic Shared and ConsumableShared wrappers

use std::time::Duration;
use turbomcp_client::{Client, SharedClient};
use turbomcp_core::{ConsumableShared, Shareable, Shared};
use turbomcp_server::{
    ElicitationCoordinator, ServerBuilder, SharedElicitationCoordinator,
};
use turbomcp_transport::{SharedTransport, StdioTransport};

/// Custom service for demonstrating generic shared wrappers
#[derive(Debug)]
struct MetricsService {
    requests: u64,
    errors: u64,
}

impl MetricsService {
    fn new() -> Self {
        Self {
            requests: 0,
            errors: 0,
        }
    }

    fn record_request(&mut self) {
        self.requests += 1;
    }

    fn record_error(&mut self) {
        self.errors += 1;
    }

    fn get_stats(&self) -> (u64, u64) {
        (self.requests, self.errors)
    }
}

/// Example demonstrating shared wrappers for concurrent access
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("TurboMCP Shared Wrappers Demo");
    println!("=============================");

    // 1. SharedTransport Example
    println!("\n1. SharedTransport Demo");
    println!("-----------------------");

    let transport = StdioTransport::new();
    let shared_transport = SharedTransport::new(transport);

    // Clone for sharing across tasks
    let transport1 = shared_transport.clone();
    let transport2 = shared_transport.clone();

    // Both tasks can access transport concurrently
    let handle1 = tokio::spawn(async move {
        let transport_type = transport1.transport_type().await;
        println!("Task 1: Transport type = {:?}", transport_type);
        transport_type
    });

    let handle2 = tokio::spawn(async move {
        let capabilities = transport2.capabilities().await;
        println!("Task 2: Transport capabilities = {:?}", capabilities);
        capabilities
    });

    let (_type, _caps) = tokio::try_join!(handle1, handle2)?;
    println!("✓ SharedTransport: Both tasks completed successfully");

    // 2. McpServer Clone Example (Axum/Tower Pattern)
    println!("\n2. McpServer Clone Demo (Axum/Tower Pattern)");
    println!("---------------------------------------------");

    let server = ServerBuilder::new()
        .name("Demo Server")
        .version("1.0.0")
        .build();

    // Clone for sharing across tasks (cheap - just Arc increments)
    let server1 = server.clone();
    let server2 = server.clone();

    // Both tasks can access server state concurrently
    let handle1 = tokio::spawn(async move {
        let config = server1.config();
        println!("Task 1: Server name = {}", config.name);
    });

    let handle2 = tokio::spawn(async move {
        let lifecycle = server2.lifecycle();
        let health = lifecycle.health().await;
        println!("Task 2: Server health = {:?}", health);
    });

    tokio::try_join!(handle1, handle2)?;
    println!("✓ McpServer Clone: Both tasks completed successfully (following Axum/Tower pattern)");

    // 3. SharedElicitationCoordinator Example
    println!("\n3. SharedElicitationCoordinator Demo");
    println!("------------------------------------");

    let coordinator = ElicitationCoordinator::with_config(Duration::from_secs(30));
    let shared_coordinator = SharedElicitationCoordinator::new(coordinator);

    // Clone for sharing across tasks
    let coord1 = shared_coordinator.clone();
    let coord2 = shared_coordinator.clone();

    // Both tasks can access coordinator concurrently
    let handle1 = tokio::spawn(async move {
        let stats = coord1.get_stats().await;
        println!("Task 1: Pending elicitations = {}", stats.pending_count);
        stats
    });

    let handle2 = tokio::spawn(async move {
        let timeout = coord2.default_timeout();
        println!("Task 2: Default timeout = {:?}", timeout);
        timeout
    });

    let (_stats, _timeout) = tokio::try_join!(handle1, handle2)?;
    println!("✓ SharedElicitationCoordinator: Both tasks completed successfully");

    // 4. SharedClient Example
    println!("\n4. SharedClient Demo");
    println!("--------------------");

    let transport = StdioTransport::new();
    let client = Client::new(transport);
    let shared_client = SharedClient::new(client);

    // Clone for sharing across tasks
    let client1 = shared_client.clone();
    let client2 = shared_client.clone();

    // Both tasks can access client concurrently
    let handle1 = tokio::spawn(async move {
        let capabilities = client1.capabilities().await;
        println!("Task 1: Client supports tools = {}", capabilities.tools);
        capabilities
    });

    let handle2 = tokio::spawn(async move {
        let capabilities = client2.capabilities().await;
        println!("Task 2: Client supports prompts = {}", capabilities.prompts);
        capabilities
    });

    let (_caps1, _caps2) = tokio::try_join!(handle1, handle2)?;
    println!("✓ SharedClient: Both tasks completed successfully");

    // 5. Generic Shared<T> Example
    println!("\n5. Generic Shared<T> Demo");
    println!("-------------------------");

    let metrics = MetricsService::new();
    let shared_metrics = Shared::new(metrics);

    // Clone for sharing across tasks
    let metrics1 = shared_metrics.clone();
    let metrics2 = shared_metrics.clone();

    // Simulate concurrent metric recording
    let handle1 = tokio::spawn(async move {
        for i in 0..5 {
            metrics1.with_mut(|m| m.record_request()).await;
            if i % 2 == 0 {
                metrics1.with_mut(|m| m.record_error()).await;
            }
        }
        println!("Task 1: Recorded 5 requests");
    });

    let handle2 = tokio::spawn(async move {
        for i in 0..3 {
            metrics2.with_mut(|m| m.record_request()).await;
            if i == 1 {
                metrics2.with_mut(|m| m.record_error()).await;
            }
        }
        println!("Task 2: Recorded 3 requests");
    });

    tokio::try_join!(handle1, handle2)?;

    let (requests, errors) = shared_metrics.with(|m| m.get_stats()).await;
    println!(
        "✓ Shared<T>: Total requests = {}, Total errors = {}",
        requests, errors
    );

    // 6. ConsumableShared<T> Example
    println!("\n6. ConsumableShared<T> Demo");
    println!("---------------------------");

    let metrics = MetricsService::new();
    let consumable_metrics = ConsumableShared::new(metrics);
    let metrics_clone = consumable_metrics.clone();

    // Record some metrics before consumption
    consumable_metrics
        .with_mut(|m| {
            m.record_request();
            m.record_request();
            m.record_error();
        })
        .await?;

    let (requests, errors) = consumable_metrics.with(|m| m.get_stats()).await?;
    println!(
        "Before consumption: {} requests, {} errors",
        requests, errors
    );

    // Consume the metrics service
    let final_metrics = consumable_metrics.consume().await?;
    let (final_requests, final_errors) = final_metrics.get_stats();
    println!(
        "After consumption: {} requests, {} errors",
        final_requests, final_errors
    );

    // Verify clone is also consumed
    assert!(!metrics_clone.is_available().await);
    println!("✓ ConsumableShared<T>: Successfully consumed service");

    println!("\n🎉 All shared wrapper demos completed successfully!");
    println!("\nKey Benefits Demonstrated:");
    println!("• Thread-safe concurrent access without exposed Arc/Mutex");
    println!("• Clone-able for easy sharing across async tasks");
    println!("• McpServer follows Axum/Tower Clone pattern (not Arc-wrapped)");
    println!("• Consistent API patterns across all wrapper types");
    println!("• Zero-cost abstractions over existing TurboMCP types");
    println!("• Maintains strict protocol compliance and semantics");

    Ok(())
}
