//! Rate limiting middleware using token bucket algorithm

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderValue, StatusCode},
    middleware::Next,
    response::Response,
};

use crate::axum::config::{RateLimitConfig, RateLimitKey};

/// Rate limiting middleware - implements basic token bucket algorithm
///
/// This is a simple implementation suitable for single-instance deployments.
/// For production distributed systems, consider using tower-governor or
/// implementing distributed rate limiting with Redis.
pub async fn rate_limiting_middleware(
    State(rate_config): State<RateLimitConfig>,
    request: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    // Extract rate limiting key based on configuration
    let rate_key = match rate_config.key_function {
        RateLimitKey::IpAddress => {
            // Extract IP from headers or connection info
            request
                .headers()
                .get("x-forwarded-for")
                .or_else(|| request.headers().get("x-real-ip"))
                .and_then(|h| h.to_str().ok())
                .unwrap_or("unknown")
                .to_string()
        }
        RateLimitKey::UserId => {
            // Extract user ID from authentication context
            request
                .extensions()
                .get::<String>()
                .cloned()
                .unwrap_or_else(|| "anonymous".to_string())
        }
        RateLimitKey::Custom => {
            // Custom key extraction logic would go here
            "custom_key".to_string()
        }
    };

    // Simple in-memory rate limiter using LazyLock
    type RateLimiterMap = Arc<Mutex<HashMap<String, (std::time::Instant, u32)>>>;
    static RATE_LIMITER: std::sync::LazyLock<RateLimiterMap> =
        std::sync::LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

    let now = std::time::Instant::now();
    let remaining_requests;

    // Scope to limit the lock duration
    {
        let mut limiter = RATE_LIMITER.lock();
        let (last_reset, count) = limiter.entry(rate_key.clone()).or_insert((now, 0));

        // Reset counter if a minute has passed
        if now.duration_since(*last_reset) >= std::time::Duration::from_secs(60) {
            *last_reset = now;
            *count = 0;
        }

        // Check rate limit
        if *count >= rate_config.requests_per_minute {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }

        // Increment counter
        *count += 1;
        remaining_requests = rate_config.requests_per_minute.saturating_sub(*count);
    }

    // Continue processing
    let mut response = next.run(request).await;

    // Add rate limit headers
    let headers = response.headers_mut();
    if let Ok(header_value) = HeaderValue::from_str(&rate_config.requests_per_minute.to_string()) {
        headers.insert("X-RateLimit-Limit", header_value);
    }
    if let Ok(header_value) = HeaderValue::from_str(&remaining_requests.to_string()) {
        headers.insert("X-RateLimit-Remaining", header_value);
    }

    Ok(response)
}
