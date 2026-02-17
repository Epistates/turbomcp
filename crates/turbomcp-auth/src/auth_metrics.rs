//! Auth-specific metrics for observability
//!
//! Enable with the `metrics` feature flag. Requires a `metrics` recorder
//! to be installed (e.g., via `metrics-exporter-prometheus`).
//!
//! ## Metrics Provided
//!
//! - `mcp_auth_attempts_total` - Counter for authentication attempts (labels: provider, status)
//! - `mcp_auth_token_validations_total` - Counter for token validation attempts (labels: provider, status, cache)
//! - `mcp_auth_token_validation_duration_seconds` - Histogram for token validation duration
//! - `mcp_auth_rate_limited_total` - Counter for rate-limited requests (labels: endpoint, key_type)
//!
//! ## Example
//!
//! ```rust,ignore
//! use turbomcp_auth::init_auth_metrics;
//!
//! // Initialize metric descriptions once at startup
//! init_auth_metrics();
//!
//! // Metrics are automatically recorded by the auth manager
//! ```

#[cfg(feature = "metrics")]
use metrics::{counter, describe_counter, describe_histogram, histogram};

#[cfg(feature = "metrics")]
use std::sync::Once;

#[cfg(feature = "metrics")]
static INIT: Once = Once::new();

/// Initialize auth metric descriptions. Call once at startup.
///
/// This function is idempotent - it's safe to call multiple times.
/// Only the first call will register the metric descriptions.
///
/// # Example
///
/// ```rust
/// use turbomcp_auth::init_auth_metrics;
///
/// // Initialize metrics once at startup
/// init_auth_metrics();
/// ```
#[cfg(feature = "metrics")]
pub fn init_auth_metrics() {
    INIT.call_once(|| {
        describe_counter!(
            "mcp_auth_attempts_total",
            "Total authentication attempts (success and failure)"
        );
        describe_counter!(
            "mcp_auth_token_validations_total",
            "Total token validation attempts (with cache hit tracking)"
        );
        describe_counter!(
            "mcp_auth_rate_limited_total",
            "Total rate-limited auth requests"
        );
        describe_histogram!(
            "mcp_auth_token_validation_duration_seconds",
            "Token validation duration in seconds"
        );
    });
}

/// Record an authentication attempt
///
/// # Arguments
///
/// * `provider` - Authentication provider name (e.g., "oauth2", "api-key")
/// * `success` - Whether the authentication succeeded
#[cfg(feature = "metrics")]
pub(crate) fn record_auth_attempt(provider: &str, success: bool) {
    let status = if success { "success" } else { "failure" };
    counter!(
        "mcp_auth_attempts_total",
        "provider" => provider.to_owned(),
        "status" => status
    )
    .increment(1);
}

/// Record a token validation attempt
///
/// # Arguments
///
/// * `provider` - Authentication provider name (e.g., "oauth2", "api-key")
/// * `success` - Whether the validation succeeded
/// * `cache_hit` - Whether the validation result came from cache
#[cfg(feature = "metrics")]
pub(crate) fn record_token_validation(provider: &str, success: bool, cache_hit: bool) {
    let status = if success { "success" } else { "failure" };
    let cache = if cache_hit { "hit" } else { "miss" };
    counter!(
        "mcp_auth_token_validations_total",
        "provider" => provider.to_owned(),
        "status" => status,
        "cache" => cache
    )
    .increment(1);
}

/// Record token validation duration
///
/// # Arguments
///
/// * `duration_seconds` - Duration of the validation operation in seconds
#[cfg(feature = "metrics")]
pub(crate) fn record_token_validation_duration(duration_seconds: f64) {
    histogram!("mcp_auth_token_validation_duration_seconds").record(duration_seconds);
}

/// Record a rate-limited request
///
/// # Arguments
///
/// * `endpoint` - Endpoint that was rate-limited (e.g., "login", "token")
/// * `key_type` - Type of rate limit key (e.g., "ip", "user", "composite")
#[cfg(feature = "metrics")]
pub(crate) fn record_rate_limited(endpoint: &str, key_type: &str) {
    counter!(
        "mcp_auth_rate_limited_total",
        "endpoint" => endpoint.to_owned(),
        "key_type" => key_type.to_owned()
    )
    .increment(1);
}

// No-op versions when metrics feature is disabled
#[cfg(not(feature = "metrics"))]
#[allow(missing_docs)]
pub fn init_auth_metrics() {}

#[cfg(not(feature = "metrics"))]
#[allow(missing_docs)]
pub(crate) fn record_auth_attempt(_provider: &str, _success: bool) {}

#[cfg(not(feature = "metrics"))]
#[allow(missing_docs)]
pub(crate) fn record_token_validation(_provider: &str, _success: bool, _cache_hit: bool) {}

#[cfg(not(feature = "metrics"))]
#[allow(missing_docs)]
pub(crate) fn record_token_validation_duration(_duration_seconds: f64) {}

#[cfg(not(feature = "metrics"))]
#[allow(missing_docs)]
pub(crate) fn record_rate_limited(_endpoint: &str, _key_type: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_auth_metrics() {
        // Should not panic even when called multiple times
        init_auth_metrics();
        init_auth_metrics();
    }

    #[test]
    fn test_no_op_functions_without_metrics() {
        // These should compile and run without errors when metrics feature is disabled
        #[cfg(not(feature = "metrics"))]
        {
            record_auth_attempt("test", true);
            record_token_validation("test", true, false);
            record_token_validation_duration(0.5);
            record_rate_limited("login", "ip");
        }
    }

    #[cfg(feature = "metrics")]
    #[test]
    fn test_record_functions_with_metrics() {
        // Initialize first
        init_auth_metrics();

        // These should compile and run without panicking
        record_auth_attempt("oauth2", true);
        record_auth_attempt("api-key", false);
        record_token_validation("oauth2", true, true);
        record_token_validation("oauth2", false, false);
        record_token_validation_duration(0.015);
        record_rate_limited("login", "ip");
        record_rate_limited("token", "composite");
    }
}
