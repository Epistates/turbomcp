//! Example of using RuntimeProxyBuilder
//!
//! This example demonstrates how to create and configure a runtime proxy
//! with comprehensive security features.

use turbomcp_proxy::prelude::*;

#[tokio::main]
async fn main() -> ProxyResult<()> {
    // Example 1: STDIO backend with HTTP frontend
    println!("Creating runtime proxy with STDIO backend...");

    let proxy_result = RuntimeProxyBuilder::new()
        .with_stdio_backend("python", vec!["server.py".to_string()])
        .with_http_frontend("127.0.0.1:3000")
        .with_timeout(60_000)?
        .build()
        .await;

    match proxy_result {
        Ok(proxy) => {
            println!("✓ Proxy created successfully");
            println!("  Backend: STDIO (python server.py)");
            println!("  Frontend: HTTP on 127.0.0.1:3000");
            println!("  Timeout: 60s");

            if let Some(metrics) = proxy.metrics() {
                println!("  Metrics enabled: yes");
                println!("  Current requests: {}", metrics.total_requests());
            }
        }
        Err(e) => {
            println!("✗ Failed to create proxy: {}", e);
            println!("  (This is expected if python or server.py doesn't exist)");
        }
    }

    println!();

    // Example 2: HTTP backend with STDIO frontend
    println!("Creating runtime proxy with HTTP backend...");

    let proxy_result = RuntimeProxyBuilder::new()
        .with_http_backend("https://api.example.com", None)
        .with_stdio_frontend()
        .build()
        .await;

    match proxy_result {
        Ok(_proxy) => {
            println!("✓ Proxy created successfully");
            println!("  Backend: HTTP (https://api.example.com)");
            println!("  Frontend: STDIO");
        }
        Err(e) => {
            println!("✗ Failed to create proxy: {}", e);
            println!("  (This is expected if the HTTP endpoint doesn't exist)");
        }
    }

    println!();

    // Example 3: Security validation examples
    println!("Security validation examples:");

    // Invalid command (not in allowlist)
    let result = RuntimeProxyBuilder::new()
        .with_stdio_backend("malicious", vec![])
        .with_stdio_frontend()
        .build()
        .await;

    match result {
        Err(e) => println!("✓ Blocked invalid command: {}", e),
        Ok(_) => println!("✗ Should have blocked invalid command"),
    }

    // HTTP without HTTPS (non-localhost)
    let result = RuntimeProxyBuilder::new()
        .with_http_backend("http://api.example.com", None)
        .with_stdio_frontend()
        .build()
        .await;

    match result {
        Err(e) => println!("✓ Blocked non-HTTPS URL: {}", e),
        Ok(_) => println!("✗ Should have blocked non-HTTPS"),
    }

    // Timeout too large
    let result = RuntimeProxyBuilder::new().with_timeout(999_999_999);

    match result {
        Err(e) => println!("✓ Blocked excessive timeout: {}", e),
        Ok(_) => println!("✗ Should have blocked excessive timeout"),
    }

    println!();

    // Example 4: Metrics demonstration
    println!("Metrics demonstration:");
    let metrics = AtomicMetrics::new();

    // Simulate some activity
    metrics.inc_requests_forwarded();
    metrics.inc_requests_forwarded();
    metrics.inc_requests_failed();
    metrics.add_bytes_sent(1024);
    metrics.add_bytes_received(2048);
    metrics.update_latency_us(5000);

    let snapshot = metrics.snapshot();
    println!("  Requests forwarded: {}", snapshot.requests_forwarded);
    println!("  Requests failed: {}", snapshot.requests_failed);
    println!(
        "  Success rate: {:.1}%",
        snapshot.success_rate().unwrap_or(0.0)
    );
    println!("  Bytes sent: {}", snapshot.bytes_sent);
    println!("  Bytes received: {}", snapshot.bytes_received);
    println!("  Average latency: {:.2}ms", snapshot.average_latency_ms);

    Ok(())
}
