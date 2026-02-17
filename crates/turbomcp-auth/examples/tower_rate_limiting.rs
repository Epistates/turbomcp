//! Example: Tower Rate Limiting Middleware
//!
//! This example demonstrates how to compose the RateLimitLayer with
//! AuthLayer to create a secure, rate-limited authentication service.
//!
//! Run with:
//! ```sh
//! cargo run --example tower_rate_limiting --features middleware
//! ```

use std::convert::Infallible;
use std::time::Duration;
use tower::ServiceBuilder;
use tower_service::Service;
use turbomcp_auth::rate_limit::{EndpointLimit, RateLimitConfig, RateLimiter};
use turbomcp_auth::tower::RateLimitLayer;

#[tokio::main]
async fn main() {
    println!("Tower Rate Limiting + Authentication Example\n");

    // 1. Create rate limiter with strict limits for demonstration
    let limiter = RateLimiter::new(
        RateLimitConfig::builder()
            .endpoint_limit(
                "token",
                EndpointLimit {
                    requests: 3,
                    window: Duration::from_secs(10),
                    burst: 1,
                },
            )
            .build(),
    );

    println!("Rate limiter configured:");
    println!("  - Endpoint: token");
    println!("  - Limit: 3 requests + 1 burst");
    println!("  - Window: 10 seconds\n");

    // 2. Create a simple inner service that handles requests
    let inner_service = tower::service_fn(|req: http::Request<()>| async move {
        let path = req.uri().path();
        println!("✅ Request allowed: {path}");
        Ok::<_, Infallible>(http::Response::new(()))
    });

    // 3. Build the middleware stack
    //    For this demo, we only show rate limiting (auth layer would require Clone on provider)
    let service = ServiceBuilder::new()
        .layer(RateLimitLayer::new(limiter))
        .service(inner_service);

    // Clone the service for concurrent requests
    let mut service = service;

    println!("Making 5 rapid requests to /token endpoint:\n");

    // 5. Demonstrate rate limiting
    for i in 1..=5 {
        let req = http::Request::builder()
            .uri("/token")
            .header("x-forwarded-for", "192.168.1.100")
            .body(())
            .unwrap();

        match service.call(req).await {
            Ok(Ok(_response)) => {
                println!("[Request {i}] ✅ Success - request allowed");
            }
            Ok(Err(rejection)) => {
                println!("[Request {i}] ⛔ Rate limited: {rejection}");
            }
            Err(_) => {
                println!("[Request {i}] ❌ Infallible error (should never happen)");
            }
        }

        // Small delay between requests
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    println!("\n✨ Example complete!");
    println!("\nKey observations:");
    println!("  - First 4 requests succeed (3 regular + 1 burst)");
    println!("  - 5th request is rate limited");
    println!("  - Each IP gets its own rate limit bucket");
    println!("  - Rate limiting works independently of authentication");
    println!("\nIn production:");
    println!("  - Use stricter limits (e.g., 5/min for login)");
    println!("  - Compose with AuthLayer for complete security");
    println!("  - Add metrics/logging for rate limit events");
    println!("  - Consider custom KeyExtractor for user-based limits");
}
