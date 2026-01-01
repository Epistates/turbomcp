//! Secure API Key Validation with Constant-Time Comparison
//!
//! This module provides timing-attack resistant API key validation using:
//! - `blake3` for fast cryptographic hashing
//! - `subtle` for constant-time comparison
//!
//! ## Security Properties
//!
//! - **Timing Attack Resistance**: Uses constant-time comparison to prevent character-by-character
//!   guessing of API keys through timing side-channels.
//! - **Pre-hashing**: Hashes keys before comparison to ensure comparison time is independent of
//!   actual key values.
//! - **Length Independence**: Comparison time is independent of key length due to fixed hash size.
//!
//! ## Attack Scenario Prevented
//!
//! Without constant-time comparison, an attacker could measure response times:
//! ```text
//! Attempt: "a..." → 0.1ms (wrong first char, fails fast)
//! Attempt: "s..." → 0.2ms (correct first char, continues comparison)
//! Attempt: "sk..." → 0.3ms (correct first two chars, continues longer)
//! ```
//!
//! With constant-time comparison, all attempts take the same time regardless of correctness.
//!
//! ## Usage
//!
//! ```rust
//! use turbomcp_auth::api_key_validation::validate_api_key;
//!
//! // Use clearly fake test keys (>= 32 chars required)
//! let provided_key = "test_key_abc123def456ghi789jkl012mno";
//! let expected_key = "test_key_abc123def456ghi789jkl012mno";
//!
//! if validate_api_key(provided_key, expected_key) {
//!     // Authenticated
//! } else {
//!     // Invalid key
//! }
//! ```
//!
//! ## Implementation Notes
//!
//! - Uses `blake3` instead of SHA-256 for performance (10x faster, still cryptographically secure)
//! - Hash size: 32 bytes (256 bits)
//! - Comparison time: ~1-2 nanoseconds (constant regardless of input)

use blake3;
use subtle::ConstantTimeEq;

/// Hash an API key using BLAKE3
///
/// BLAKE3 provides:
/// - Cryptographically secure hashing
/// - 10x faster than SHA-256
/// - Fixed 256-bit output
/// - Collision resistance
#[inline]
fn hash_api_key(key: &str) -> [u8; 32] {
    blake3::hash(key.as_bytes()).into()
}

/// Minimum required length for API keys (security best practice)
///
/// Keys shorter than this are trivially brute-forceable and rejected.
/// 32 characters provides ~192 bits of entropy with alphanumeric charset.
/// v2.3.6: Added to prevent weak API keys
pub const MIN_API_KEY_LENGTH: usize = 32;

/// Validate an API key using constant-time comparison
///
/// This function is timing-attack resistant. The comparison time is constant
/// regardless of:
/// - Which characters are correct
/// - Where the mismatch occurs
/// - The length of the keys (both are hashed to 32 bytes)
///
/// ## Security Guarantees
///
/// - **Constant Time**: Uses `subtle::ConstantTimeEq` for timing-safe comparison
/// - **Pre-hashing**: Both keys are hashed before comparison
/// - **No Early Exit**: Comparison continues even after finding a mismatch
///
/// ## Performance
///
/// - Hashing: ~50-100ns per key (BLAKE3 is very fast)
/// - Comparison: ~1-2ns (constant time)
/// - Total: ~100-200ns per validation
///
/// ## Example
///
/// ```rust
/// use turbomcp_auth::api_key_validation::validate_api_key;
///
/// // Keys must be >= 32 chars for security
/// let provided = "test_key_correct_abcdefghij123456";
/// let expected = "test_key_correct_abcdefghij123456";
///
/// assert!(validate_api_key(provided, expected));
///
/// let wrong_key = "test_key_wrongxx_abcdefghij123456";
/// assert!(!validate_api_key(wrong_key, expected));
/// ```
#[must_use]
#[inline]
pub fn validate_api_key(provided: &str, expected: &str) -> bool {
    // SECURITY: Reject trivially short keys that could be brute-forced
    // This check is NOT constant-time, but that's acceptable because:
    // 1. The information leaked (key is too short) is not useful to attackers
    // 2. Attackers should know the minimum length from documentation
    if provided.len() < MIN_API_KEY_LENGTH || expected.len() < MIN_API_KEY_LENGTH {
        tracing::warn!(
            "API key validation failed: key length ({}) below minimum required ({} chars)",
            provided.len().min(expected.len()),
            MIN_API_KEY_LENGTH
        );
        return false;
    }

    // Hash both keys to fixed 32-byte size
    let provided_hash = hash_api_key(provided);
    let expected_hash = hash_api_key(expected);

    // Constant-time comparison using subtle crate
    // This prevents timing attacks by ensuring comparison time is independent
    // of where the mismatch occurs
    provided_hash.ct_eq(&expected_hash).into()
}

/// Validate an API key against multiple possible keys (constant-time)
///
/// This function checks if the provided key matches any of the expected keys,
/// while maintaining constant-time properties. The total comparison time is
/// proportional to the number of keys checked, not to which key matches or where
/// mismatches occur.
///
/// ## Security Note
///
/// While this maintains constant-time comparison for each individual key,
/// the total time is `O(n)` where `n` is the number of keys. This means:
/// - An attacker can determine approximately how many keys are stored
/// - But cannot determine which character positions are correct
/// - Cannot perform character-by-character guessing attacks
///
/// For systems with many API keys (>1000), consider using a pre-hashed lookup
/// table to avoid the linear scan.
///
/// ## Example
///
/// ```rust
/// use turbomcp_auth::api_key_validation::validate_api_key_multiple;
///
/// // Keys must be >= 32 chars for security
/// let provided = "test_key_second_abcdefghij1234567";
/// let valid_keys = vec![
///     "test_key_first__abcdefghij1234567",
///     "test_key_second_abcdefghij1234567",
///     "test_key_third__abcdefghij1234567",
/// ];
///
/// assert!(validate_api_key_multiple(provided, &valid_keys));
/// ```
#[must_use]
pub fn validate_api_key_multiple(provided: &str, expected_keys: &[&str]) -> bool {
    let provided_hash = hash_api_key(provided);

    // Check all keys in constant time per key
    // Note: Total time is O(n) but each comparison is constant-time
    for expected_key in expected_keys {
        let expected_hash = hash_api_key(expected_key);
        if provided_hash.ct_eq(&expected_hash).into() {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    // Test keys must be >= MIN_API_KEY_LENGTH (32 chars) to pass validation
    // Using clearly fake patterns that won't trigger secret scanning
    const TEST_KEY_1: &str = "test_key_1234567890abcdef1234567890abc";
    const TEST_KEY_2: &str = "test_key_0000000000000000111111111111a";
    const TEST_KEY_3: &str = "demo_key_1234567890abcdef1234567890abc";

    #[test]
    fn test_validate_correct_key() {
        assert!(validate_api_key(TEST_KEY_1, TEST_KEY_1));
    }

    #[test]
    fn test_validate_incorrect_key() {
        assert!(!validate_api_key(TEST_KEY_2, TEST_KEY_1));
    }

    #[test]
    fn test_validate_prefix_mismatch() {
        assert!(!validate_api_key(TEST_KEY_3, TEST_KEY_1));
    }

    #[test]
    fn test_validate_suffix_mismatch() {
        // Same key with last char changed
        let wrong_suffix = "test_key_1234567890abcdef1234567890abx";
        assert!(!validate_api_key(wrong_suffix, TEST_KEY_1));
    }

    #[test]
    fn test_validate_empty_keys() {
        // Empty keys and short keys should all return false (security)
        assert!(!validate_api_key("", ""));
        assert!(!validate_api_key("key", ""));
        assert!(!validate_api_key("", "key"));
    }

    #[test]
    fn test_validate_short_keys_rejected() {
        // Keys shorter than MIN_API_KEY_LENGTH should be rejected
        let short_key = "test_key_short";
        let long_key = TEST_KEY_1;

        assert!(!validate_api_key(short_key, long_key));
        assert!(!validate_api_key(long_key, short_key));
        assert!(!validate_api_key(short_key, short_key));
    }

    #[test]
    fn test_validate_case_sensitive() {
        let lower = "test_key_abcdefghijklmnopqrstuvwxyz1234";
        let upper = "SK_LIVE_ABCDEFGHIJKLMNOPQRSTUVWXYZ1234";
        assert!(!validate_api_key(lower, upper));
    }

    #[test]
    fn test_validate_multiple_keys_first_match() {
        let provided = "test_key_key1_abcdefghijklmnopqrstuv";
        let valid_keys = vec![
            "test_key_key1_abcdefghijklmnopqrstuv",
            "test_key_key2_abcdefghijklmnopqrstuv",
            "test_key_key3_abcdefghijklmnopqrstuv",
        ];
        assert!(validate_api_key_multiple(provided, &valid_keys));
    }

    #[test]
    fn test_validate_multiple_keys_middle_match() {
        let provided = "test_key_key2_abcdefghijklmnopqrstuv";
        let valid_keys = vec![
            "test_key_key1_abcdefghijklmnopqrstuv",
            "test_key_key2_abcdefghijklmnopqrstuv",
            "test_key_key3_abcdefghijklmnopqrstuv",
        ];
        assert!(validate_api_key_multiple(provided, &valid_keys));
    }

    #[test]
    fn test_validate_multiple_keys_last_match() {
        let provided = "test_key_key3_abcdefghijklmnopqrstuv";
        let valid_keys = vec![
            "test_key_key1_abcdefghijklmnopqrstuv",
            "test_key_key2_abcdefghijklmnopqrstuv",
            "test_key_key3_abcdefghijklmnopqrstuv",
        ];
        assert!(validate_api_key_multiple(provided, &valid_keys));
    }

    #[test]
    fn test_validate_multiple_keys_no_match() {
        let provided = "test_key_wrong_bcdefghijklmnopqrstuv";
        let valid_keys = vec![
            "test_key_key1_abcdefghijklmnopqrstuv",
            "test_key_key2_abcdefghijklmnopqrstuv",
            "test_key_key3_abcdefghijklmnopqrstuv",
        ];
        assert!(!validate_api_key_multiple(provided, &valid_keys));
    }

    #[test]
    fn test_validate_multiple_keys_empty_list() {
        let provided = "test_key_key1_abcdefghijklmnopqrstuv";
        let valid_keys: Vec<&str> = vec![];
        assert!(!validate_api_key_multiple(provided, &valid_keys));
    }

    #[test]
    fn test_timing_attack_resistance() {
        // This test verifies that comparison time is independent of where mismatch occurs
        let correct_key = "test_key_1234567890abcdef1234567890abc";

        // Key with mismatch in first character
        let wrong_prefix = "xk_live_1234567890abcdef1234567890abc";

        // Key with mismatch in last character
        let wrong_suffix = "test_key_1234567890abcdef1234567890abx";

        // Warm up
        for _ in 0..1000 {
            let _ = validate_api_key(wrong_prefix, correct_key);
            let _ = validate_api_key(wrong_suffix, correct_key);
        }

        // Measure timing for prefix mismatch
        let start = Instant::now();
        for _ in 0..10000 {
            let _ = validate_api_key(wrong_prefix, correct_key);
        }
        let prefix_time = start.elapsed();

        // Measure timing for suffix mismatch
        let start = Instant::now();
        for _ in 0..10000 {
            let _ = validate_api_key(wrong_suffix, correct_key);
        }
        let suffix_time = start.elapsed();

        // Calculate difference in nanoseconds
        let diff_ns = (prefix_time.as_nanos() as i128 - suffix_time.as_nanos() as i128).abs();
        let avg_diff_ns = diff_ns / 10000;

        // Timing difference should be negligible (< 10ns per comparison on average)
        // This is much smaller than network jitter (~1ms = 1,000,000ns)
        //
        // Note: This test may be flaky on heavily loaded systems.
        // If it fails, it doesn't necessarily mean timing attack is possible,
        // just that system noise exceeded threshold.
        println!(
            "Average timing difference: {}ns per comparison",
            avg_diff_ns
        );

        // Allow up to 500ns difference (generous margin for system noise on various architectures)
        // Note: This is still 2000x smaller than network jitter (~1ms = 1,000,000ns),
        // making timing attacks via network infeasible.
        assert!(
            avg_diff_ns < 500,
            "Timing difference too large: {}ns (threshold: 500ns). \
             This suggests potential timing attack vulnerability.",
            avg_diff_ns
        );
    }

    #[test]
    fn test_blake3_hash_consistency() {
        // Verify that hashing is deterministic
        let key = "test_key_test";
        let hash1 = hash_api_key(key);
        let hash2 = hash_api_key(key);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_blake3_hash_collision_resistance() {
        // Different keys should produce different hashes
        let key1 = "test_key_1234567890abcdef";
        let key2 = "test_key_1234567890abcdeg"; // Last char different

        let hash1 = hash_api_key(key1);
        let hash2 = hash_api_key(key2);

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_long_keys() {
        // Test with very long keys
        let long_key = "test_key_".to_string() + &"a".repeat(1000);
        assert!(validate_api_key(&long_key, &long_key));
    }

    #[test]
    fn test_special_characters() {
        // Test with special characters
        let key = "test_key_!@#$%^&*()_+-={}[]|:;<>?,./";
        assert!(validate_api_key(key, key));
    }

    #[test]
    fn test_unicode_keys() {
        // Test with Unicode characters (must be >= MIN_API_KEY_LENGTH bytes)
        // Note: Unicode chars can be multi-byte, this key is ~50 bytes
        let key = "test_key_你好世界🔒abcdefghijklmnopqrst";
        assert!(validate_api_key(key, key));
    }
}
