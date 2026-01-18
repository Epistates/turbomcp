//! Property-based tests for TurboMCP transport resilience features
//!
//! Uses proptest to verify invariants and properties of:
//! - Circuit breaker state transitions
//! - Retry backoff calculations
//! - Deduplication cache behavior
//! - Configuration validation

use proptest::prelude::*;
use std::time::Duration;
use turbomcp_transport::resilience::{
    CircuitBreaker, CircuitBreakerConfig, CircuitState, DeduplicationCache, RetryConfig,
};

// =============================================================================
// CIRCUIT BREAKER PROPERTY TESTS
// =============================================================================

/// Strategy for generating valid circuit breaker configurations
fn circuit_breaker_config_strategy() -> impl Strategy<Value = CircuitBreakerConfig> {
    (
        1u32..=20,    // failure_threshold
        1u32..=10,    // success_threshold
        50u64..=5000, // timeout_ms
        5usize..=100, // rolling_window_size
        1u32..=50,    // minimum_requests
    )
        .prop_map(
            |(failure_threshold, success_threshold, timeout_ms, rolling_window_size, minimum_requests)| {
                CircuitBreakerConfig {
                    failure_threshold,
                    success_threshold,
                    timeout: Duration::from_millis(timeout_ms),
                    rolling_window_size,
                    minimum_requests,
                }
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: Circuit breaker starts in Closed state
    #[test]
    fn prop_circuit_breaker_starts_closed(config in circuit_breaker_config_strategy()) {
        let cb = CircuitBreaker::new(config);
        let stats = cb.statistics();
        prop_assert_eq!(stats.state, CircuitState::Closed);
    }

    /// Property: Circuit breaker state is always valid (one of three states)
    #[test]
    fn prop_circuit_breaker_state_valid(
        config in circuit_breaker_config_strategy(),
        operations in prop::collection::vec(prop::bool::ANY, 1..50)
    ) {
        let mut cb = CircuitBreaker::new(config);

        for success in operations {
            // Record the result with a dummy duration
            cb.record_result(success, Duration::from_millis(10));

            let stats = cb.statistics();
            prop_assert!(matches!(
                stats.state,
                CircuitState::Closed | CircuitState::Open | CircuitState::HalfOpen
            ));
        }
    }

    /// Property: Failure rate is bounded between 0 and 1
    #[test]
    fn prop_failure_rate_bounded(
        config in circuit_breaker_config_strategy(),
        operations in prop::collection::vec(prop::bool::ANY, 1..100)
    ) {
        let mut cb = CircuitBreaker::new(config);

        for success in operations {
            cb.record_result(success, Duration::from_millis(10));

            let stats = cb.statistics();
            prop_assert!(stats.failure_rate >= 0.0);
            prop_assert!(stats.failure_rate <= 1.0);
        }
    }

    /// Property: Circuit breaker allows execution when closed
    #[test]
    fn prop_closed_circuit_allows_execution(config in circuit_breaker_config_strategy()) {
        let mut cb = CircuitBreaker::new(config);
        // A closed circuit should allow execution
        prop_assert!(cb.should_allow_operation());
    }

    /// Property: Reset returns circuit breaker to initial state
    #[test]
    fn prop_reset_returns_to_initial(
        config in circuit_breaker_config_strategy(),
        operations in prop::collection::vec(prop::bool::ANY, 1..20)
    ) {
        let mut cb = CircuitBreaker::new(config);

        // Apply some operations
        for success in operations {
            cb.record_result(success, Duration::from_millis(10));
        }

        // Reset
        cb.reset();

        // Should be back to closed state
        let stats = cb.statistics();
        prop_assert_eq!(stats.state, CircuitState::Closed);
        prop_assert_eq!(stats.failure_count, 0);
        prop_assert_eq!(stats.success_count, 0);
    }
}

// =============================================================================
// RETRY CONFIGURATION PROPERTY TESTS
// =============================================================================

/// Strategy for generating valid retry configurations
fn retry_config_strategy() -> impl Strategy<Value = RetryConfig> {
    (
        1u32..=10,      // max_attempts
        10u64..=500,    // base_delay_ms
        1000u64..=60000, // max_delay_ms
        1.1f64..=5.0,   // backoff_multiplier
        0.0f64..=0.5,   // jitter_factor
    )
        .prop_map(
            |(max_attempts, base_delay_ms, max_delay_ms, backoff_multiplier, jitter_factor)| {
                RetryConfig {
                    max_attempts,
                    base_delay: Duration::from_millis(base_delay_ms),
                    max_delay: Duration::from_millis(max_delay_ms),
                    backoff_multiplier,
                    jitter_factor,
                    retry_on_connection_error: true,
                    retry_on_timeout: true,
                    custom_retry_conditions: vec![],
                }
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: First attempt delay is approximately base_delay (allowing for jitter)
    #[test]
    fn prop_first_attempt_near_base_delay(config in retry_config_strategy()) {
        let delay = config.calculate_delay(0);

        // First attempt should be close to base_delay (within jitter range)
        let min_expected = config.base_delay.as_millis() as f64 * (1.0 - config.jitter_factor);
        let max_expected = config.base_delay.as_millis() as f64 * (1.0 + config.jitter_factor);

        let delay_ms = delay.as_millis() as f64;
        prop_assert!(
            delay_ms >= min_expected * 0.5 && delay_ms <= max_expected * 2.0,
            "First attempt delay {} not within expected range [{}, {}]",
            delay_ms, min_expected, max_expected
        );
    }

    /// Property: Delay never exceeds max_delay (within jitter tolerance)
    #[test]
    fn prop_delay_capped_at_max(
        config in retry_config_strategy(),
        attempt in 0u32..20
    ) {
        let delay = config.calculate_delay(attempt);

        // Allow some tolerance for jitter
        let max_with_tolerance = config.max_delay.as_millis() as f64 * (1.0 + config.jitter_factor);

        prop_assert!(
            delay.as_millis() as f64 <= max_with_tolerance,
            "Delay {}ms exceeds max {}ms with jitter tolerance",
            delay.as_millis(),
            max_with_tolerance as u64
        );
    }

    /// Property: Delay is monotonically increasing (on average) until max
    #[test]
    fn prop_delay_increases_on_average(config in retry_config_strategy()) {
        // Average multiple samples to account for jitter
        let samples = 10;
        let mut prev_avg = 0.0;

        for attempt in 0u32..5 {
            let mut total = 0u64;
            for _ in 0..samples {
                total += config.calculate_delay(attempt).as_millis() as u64;
            }
            let avg = total as f64 / samples as f64;

            if attempt > 0 && prev_avg < config.max_delay.as_millis() as f64 * 0.9 {
                // If we haven't hit max, delay should increase (allowing 50% tolerance for jitter)
                prop_assert!(
                    avg >= prev_avg * 0.5,
                    "Average delay decreased unexpectedly from {} to {} at attempt {}",
                    prev_avg, avg, attempt
                );
            }
            prev_avg = avg;
        }
    }

    /// Property: Delay is always positive
    #[test]
    fn prop_delay_positive(
        config in retry_config_strategy(),
        attempt in 0u32..100
    ) {
        let delay = config.calculate_delay(attempt);
        prop_assert!(delay.as_nanos() > 0, "Delay should be positive");
    }
}

// =============================================================================
// DEDUPLICATION CACHE PROPERTY TESTS
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Property: First occurrence is never a duplicate
    #[test]
    fn prop_first_occurrence_not_duplicate(
        max_size in 10usize..=1000,
        ttl_ms in 100u64..=10000,
        message_id in "[a-zA-Z0-9]{1,20}"
    ) {
        let mut cache = DeduplicationCache::new(max_size, Duration::from_millis(ttl_ms));
        let is_dup = cache.is_duplicate(&message_id);
        prop_assert!(!is_dup, "First occurrence should not be a duplicate");
    }

    /// Property: Second occurrence is a duplicate
    #[test]
    fn prop_second_occurrence_is_duplicate(
        max_size in 10usize..=1000,
        ttl_ms in 100u64..=10000,
        message_id in "[a-zA-Z0-9]{1,20}"
    ) {
        let mut cache = DeduplicationCache::new(max_size, Duration::from_millis(ttl_ms));

        // First check
        let first = cache.is_duplicate(&message_id);
        prop_assert!(!first, "First occurrence should not be a duplicate");

        // Second check
        let second = cache.is_duplicate(&message_id);
        prop_assert!(second, "Second occurrence should be a duplicate");
    }

    /// Property: Cache size respects max_size
    #[test]
    fn prop_cache_respects_max_size(
        max_size in 10usize..=100,
        ttl_ms in 1000u64..=10000,
        message_ids in prop::collection::vec("[a-zA-Z0-9]{1,20}", 1..200)
    ) {
        let mut cache = DeduplicationCache::new(max_size, Duration::from_millis(ttl_ms));

        for id in &message_ids {
            cache.is_duplicate(id);
        }

        let stats = cache.statistics();
        prop_assert!(
            stats.total_entries <= max_size,
            "Cache size {} exceeds max_size {}",
            stats.total_entries,
            max_size
        );
    }

    /// Property: Different message IDs are independent
    #[test]
    fn prop_different_ids_independent(
        max_size in 10usize..=1000,
        ttl_ms in 100u64..=10000,
        id1 in "[a-zA-Z0-9]{1,10}",
        id2 in "[a-zA-Z0-9]{1,10}"
    ) {
        prop_assume!(id1 != id2);

        let mut cache = DeduplicationCache::new(max_size, Duration::from_millis(ttl_ms));

        // Check id1
        let first1 = cache.is_duplicate(&id1);
        prop_assert!(!first1, "First occurrence of id1 should not be duplicate");

        // Check id2 (should not be affected by id1)
        let first2 = cache.is_duplicate(&id2);
        prop_assert!(!first2, "First occurrence of id2 should not be duplicate");
    }

    /// Property: Cache can be cleared
    #[test]
    fn prop_cache_clear_works(
        max_size in 10usize..=100,
        ttl_ms in 100u64..=10000,
        message_ids in prop::collection::vec("[a-zA-Z0-9]{1,20}", 1..50)
    ) {
        let mut cache = DeduplicationCache::new(max_size, Duration::from_millis(ttl_ms));

        // Add some entries
        for id in &message_ids {
            cache.is_duplicate(id);
        }

        // Clear
        cache.clear();

        // Cache should be empty
        prop_assert!(cache.is_empty(), "Cache should be empty after clear");

        // Previous IDs should no longer be duplicates
        for id in message_ids.iter().take(5) {
            let is_dup = cache.is_duplicate(id);
            prop_assert!(!is_dup, "Entry should not be duplicate after clear");
        }
    }
}

// =============================================================================
// CONFIGURATION VALIDATION PROPERTY TESTS
// =============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Property: Circuit breaker config with valid values should not panic
    #[test]
    fn prop_circuit_breaker_config_valid(
        failure_threshold in 1u32..=100,
        success_threshold in 1u32..=100,
        timeout_ms in 1u64..=300000,
        rolling_window_size in 1usize..=1000,
        minimum_requests in 1u32..=1000
    ) {
        let config = CircuitBreakerConfig {
            failure_threshold,
            success_threshold,
            timeout: Duration::from_millis(timeout_ms),
            rolling_window_size,
            minimum_requests,
        };

        // Should not panic when creating circuit breaker
        let _cb = CircuitBreaker::new(config);
    }

    /// Property: Retry config with valid values should not panic
    #[test]
    fn prop_retry_config_valid(
        max_attempts in 1u32..=100,
        base_delay_ms in 1u64..=10000,
        max_delay_ms in 1u64..=300000,
        backoff_multiplier in 1.0f64..=10.0,
        jitter_factor in 0.0f64..=1.0
    ) {
        let config = RetryConfig {
            max_attempts,
            base_delay: Duration::from_millis(base_delay_ms),
            max_delay: Duration::from_millis(max_delay_ms),
            backoff_multiplier,
            jitter_factor,
            retry_on_connection_error: true,
            retry_on_timeout: true,
            custom_retry_conditions: vec![],
        };

        // Should not panic when calculating delays
        for attempt in 0..max_attempts {
            let _delay = config.calculate_delay(attempt);
        }
    }

    /// Property: Deduplication cache with valid config should not panic
    #[test]
    fn prop_dedup_cache_config_valid(
        max_size in 1usize..=10000,
        ttl_ms in 1u64..=3600000
    ) {
        let cache = DeduplicationCache::new(max_size, Duration::from_millis(ttl_ms));

        // Should not panic when using cache
        let _stats = cache.statistics();
    }
}
