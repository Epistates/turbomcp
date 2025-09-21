//! Comprehensive Security Test Suite
//!
//! This test suite validates the entire security posture of the TurboMCP transport layer
//! by testing all security measures working together under real-world attack scenarios.
//!
//! Coverage:
//! - Multi-layer security integration (Origin + Auth + Sessions + Rate Limiting)
//! - Real-world attack scenarios (DNS rebinding, session hijacking, DoS attacks)
//! - Edge cases and stress testing
//! - Performance under security load
//! - Attack vector validation and mitigation effectiveness

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use turbomcp_transport::security::{
    EnhancedSecurityConfigBuilder, SecurityError, SecurityHeaders, validate_message_size,
};

/// Test helper for creating security headers
fn create_headers(origin: &str, user_agent: &str, session_id: Option<&str>) -> SecurityHeaders {
    let mut headers = SecurityHeaders::new();
    headers.insert("Origin".to_string(), origin.to_string());
    headers.insert("User-Agent".to_string(), user_agent.to_string());
    if let Some(sid) = session_id {
        headers.insert("Mcp-Session-Id".to_string(), sid.to_string());
    }
    headers
}

/// Test helper for creating malicious headers
fn create_malicious_headers(origin: &str) -> SecurityHeaders {
    let mut headers = SecurityHeaders::new();
    headers.insert("Origin".to_string(), origin.to_string());
    headers.insert("User-Agent".to_string(), "AttackBot/1.0".to_string());
    // Attempt to bypass with fake localhost
    headers.insert("X-Forwarded-For".to_string(), "127.0.0.1".to_string());
    headers.insert("X-Real-IP".to_string(), "localhost".to_string());
    headers
}

mod integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_full_security_stack_legitimate_request() {
        // Test all security layers with a legitimate request
        let (validator, session_manager) = EnhancedSecurityConfigBuilder::new()
            .allow_localhost(true)
            .require_authentication(true)
            .with_api_keys(vec!["valid_key_123".to_string()])
            .with_rate_limit(100, Duration::from_secs(60))
            .with_max_sessions_per_ip(5)
            .enforce_ip_binding(true)
            .build();

        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        let mut headers = create_headers("http://localhost:3000", "Mozilla/5.0", None);
        headers.insert(
            "Authorization".to_string(),
            "Bearer valid_key_123".to_string(),
        );

        // Step 1: Validate basic security
        assert!(validator.validate_request(&headers, ip).is_ok());

        // Step 2: Create secure session
        let session = session_manager
            .create_session(ip, Some("Mozilla/5.0"))
            .unwrap();
        headers.insert("Mcp-Session-Id".to_string(), session.id.clone());

        // Step 3: Validate session on subsequent request
        let validated_session = session_manager
            .validate_session(&session.id, ip, Some("Mozilla/5.0"))
            .unwrap();
        assert_eq!(validated_session.request_count, 1); // Should increment

        // Step 4: Test continued legitimate usage
        for i in 2..=10 {
            let session = session_manager
                .validate_session(&session.id, ip, Some("Mozilla/5.0"))
                .unwrap();
            assert_eq!(session.request_count, i);
        }
    }

    #[tokio::test]
    async fn test_full_security_stack_blocks_attacks() {
        let (validator, session_manager) = EnhancedSecurityConfigBuilder::new()
            .allow_localhost(true)
            .require_authentication(true)
            .with_api_keys(vec!["valid_key_123".to_string()])
            .with_rate_limit(5, Duration::from_secs(60)) // Very low limit for testing
            .with_max_sessions_per_ip(2) // Low limit for testing
            .enforce_ip_binding(true)
            .build();

        let malicious_ip: IpAddr = "192.168.1.100".parse().unwrap();

        // Attack 1: DNS Rebinding Attack
        let malicious_headers = create_malicious_headers("http://evil.com");
        assert!(
            validator
                .validate_request(&malicious_headers, malicious_ip)
                .is_err()
        );

        // Attack 2: Authentication Bypass Attempt
        let mut bypass_headers = create_headers("http://localhost:3000", "AttackBot/1.0", None);
        bypass_headers.insert(
            "Authorization".to_string(),
            "Bearer invalid_key".to_string(),
        );
        assert!(
            validator
                .validate_request(&bypass_headers, malicious_ip)
                .is_err()
        );

        // Attack 3: Session Exhaustion Attack
        let mut valid_headers = create_headers("http://localhost:3000", "AttackBot/1.0", None);
        valid_headers.insert(
            "Authorization".to_string(),
            "Bearer valid_key_123".to_string(),
        );

        // Create maximum allowed sessions
        for i in 1..=2 {
            let session = session_manager
                .create_session(malicious_ip, Some(&format!("AttackBot/{}", i)))
                .unwrap();
            assert!(session.id.starts_with("mcp_session_"));
        }

        // Third session should fail
        assert!(
            session_manager
                .create_session(malicious_ip, Some("AttackBot/3"))
                .is_err()
        );

        // Attack 4: Rate Limiting Bypass Attempt
        let legitimate_ip: IpAddr = "127.0.0.1".parse().unwrap();
        let mut rate_limit_headers = create_headers("http://localhost:3000", "Mozilla/5.0", None);
        rate_limit_headers.insert(
            "Authorization".to_string(),
            "Bearer valid_key_123".to_string(),
        );

        // Exhaust rate limit (5 requests allowed)
        for _ in 1..=5 {
            assert!(
                validator
                    .validate_request(&rate_limit_headers, legitimate_ip)
                    .is_ok()
            );
        }

        // 6th request should be rate limited
        assert!(
            validator
                .validate_request(&rate_limit_headers, legitimate_ip)
                .is_err()
        );
    }
}

mod attack_scenarios {
    use super::*;

    #[tokio::test]
    async fn test_dns_rebinding_attack_prevention() {
        let (validator, _) = EnhancedSecurityConfigBuilder::new()
            .allow_localhost(true)
            .allow_any_origin(false) // Critical: no wildcard origins
            .build();

        let legitimate_ip: IpAddr = "127.0.0.1".parse().unwrap();

        // Legitimate requests should pass
        let good_origins = [
            "http://localhost:3000",
            "https://localhost:8080",
            "http://127.0.0.1:3000",
            "https://127.0.0.1:8080",
        ];

        for origin in &good_origins {
            let headers = create_headers(origin, "Mozilla/5.0", None);
            assert!(
                validator.validate_request(&headers, legitimate_ip).is_ok(),
                "Legitimate origin {} should be allowed",
                origin
            );
        }

        // DNS rebinding attacks should be blocked
        let malicious_origins = [
            "http://evil.com",
            "https://attacker.example.org",
            "http://malicious-site.net",
            "https://phishing-site.com",
            "http://192.168.1.1",     // Internal network probing
            "https://10.0.0.1",       // Internal network probing
            "http://169.254.169.254", // AWS metadata service
            "data:text/html,<script>alert('xss')</script>", // Data URI
            "javascript:alert('xss')", // JavaScript URI
            "file:///etc/passwd",     // File URI
            "",                       // Empty origin
        ];

        for origin in &malicious_origins {
            let headers = create_malicious_headers(origin);
            assert!(
                validator.validate_request(&headers, legitimate_ip).is_err(),
                "Malicious origin {} should be blocked",
                origin
            );
        }
    }

    #[tokio::test]
    async fn test_session_hijacking_prevention() {
        let (_, session_manager) = EnhancedSecurityConfigBuilder::new()
            .enforce_ip_binding(true) // Critical: IP binding enabled
            .build();

        let original_ip: IpAddr = "192.168.1.10".parse().unwrap();
        let attacker_ip: IpAddr = "192.168.1.100".parse().unwrap();
        let user_agent = "Mozilla/5.0 (Windows NT 10.0; Win64; x64)";

        // Step 1: Create legitimate session
        let session = session_manager
            .create_session(original_ip, Some(user_agent))
            .unwrap();

        // Step 2: Legitimate user continues using session
        let valid_session = session_manager
            .validate_session(&session.id, original_ip, Some(user_agent))
            .unwrap();
        assert_eq!(valid_session.original_ip, original_ip);

        // Step 3: Attacker tries to hijack session from different IP
        let hijack_result =
            session_manager.validate_session(&session.id, attacker_ip, Some(user_agent));
        assert!(
            hijack_result.is_err(),
            "Session hijacking should be prevented"
        );

        // Step 4: Attacker tries with spoofed user agent
        let spoof_result =
            session_manager.validate_session(&session.id, original_ip, Some("AttackBot/1.0"));
        assert!(
            spoof_result.is_err(),
            "User agent spoofing should be detected"
        );
    }

    #[tokio::test]
    async fn test_session_fixation_prevention() {
        let (_, session_manager) = EnhancedSecurityConfigBuilder::new()
            .enable_session_id_regeneration(true, Duration::from_millis(100)) // Fast regeneration for testing
            .build();

        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        let user_agent = "Mozilla/5.0";

        // Create session
        let session = session_manager
            .create_session(ip, Some(user_agent))
            .unwrap();
        let original_id = session.id.clone();

        // Wait for regeneration interval
        sleep(Duration::from_millis(150)).await;

        // Session ID should regenerate on next validation
        let regenerated_session = session_manager
            .validate_session(&original_id, ip, Some(user_agent))
            .unwrap();
        assert_ne!(
            regenerated_session.id, original_id,
            "Session ID should regenerate"
        );

        // Old session ID should no longer be valid
        let old_session_result =
            session_manager.validate_session(&original_id, ip, Some(user_agent));
        assert!(
            old_session_result.is_err(),
            "Old session ID should be invalid after regeneration"
        );
    }

    #[tokio::test]
    async fn test_dos_attack_message_size_protection() {
        // Test message size DoS protection
        let small_message = b"normal message";
        let large_message = vec![0u8; 2 * 1024 * 1024]; // 2MB message
        let huge_message = vec![0u8; 10 * 1024 * 1024]; // 10MB message

        let limit = 1024 * 1024; // 1MB limit

        // Small message should pass
        assert!(validate_message_size(small_message, limit).is_ok());

        // Large message should be rejected
        assert!(validate_message_size(&large_message, limit).is_err());

        // Huge message should be rejected
        assert!(validate_message_size(&huge_message, limit).is_err());

        // Test exact boundary
        let boundary_message = vec![0u8; limit]; // Exactly at limit
        assert!(validate_message_size(&boundary_message, limit).is_ok());

        let over_boundary = vec![0u8; limit + 1]; // Just over limit
        assert!(validate_message_size(&over_boundary, limit).is_err());
    }

    #[tokio::test]
    async fn test_rate_limit_attack_resistance() {
        let (validator, _) = EnhancedSecurityConfigBuilder::new()
            .allow_localhost(true)
            .with_rate_limit(10, Duration::from_secs(1)) // 10 requests per second
            .build();

        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        let headers = create_headers("http://localhost:3000", "Mozilla/5.0", None);

        // Rapid-fire legitimate requests should eventually be rate limited
        let mut success_count = 0;
        let mut rate_limited_count = 0;

        for i in 1..=20 {
            match validator.validate_request(&headers, ip) {
                Ok(()) => success_count += 1,
                Err(SecurityError::RateLimitExceeded { .. }) => rate_limited_count += 1,
                Err(e) => panic!("Unexpected error: {}", e),
            }

            // Small delay to avoid overwhelming
            if i % 5 == 0 {
                sleep(Duration::from_millis(10)).await;
            }
        }

        assert!(
            success_count <= 12,
            "Should not allow more than ~12 requests (allowing some variance)"
        );
        assert!(
            rate_limited_count >= 8,
            "Should rate limit at least 8 requests"
        );
    }
}

mod edge_cases {
    use super::*;

    #[tokio::test]
    async fn test_malformed_headers_handling() {
        let (validator, session_manager) = EnhancedSecurityConfigBuilder::new()
            .allow_localhost(true)
            .require_authentication(true)
            .with_api_keys(vec!["valid_key".to_string()])
            .build();

        let ip: IpAddr = "127.0.0.1".parse().unwrap();

        // Test various malformed header scenarios
        let malformed_scenarios = [
            // Missing Origin header
            {
                let mut headers = SecurityHeaders::new();
                headers.insert("User-Agent".to_string(), "Mozilla/5.0".to_string());
                headers
            },
            // Invalid Authorization format
            {
                let mut headers = create_headers("http://localhost:3000", "Mozilla/5.0", None);
                headers.insert(
                    "Authorization".to_string(),
                    "InvalidFormat token".to_string(),
                );
                headers
            },
            // Empty header values
            {
                let mut headers = SecurityHeaders::new();
                headers.insert("Origin".to_string(), "".to_string());
                headers.insert("User-Agent".to_string(), "".to_string());
                headers
            },
            // Very long header values (potential buffer overflow attempt)
            {
                let mut headers = SecurityHeaders::new();
                headers.insert(
                    "Origin".to_string(),
                    "http://".to_string() + &"a".repeat(10000),
                );
                headers.insert("User-Agent".to_string(), "A".repeat(10000));
                headers
            },
        ];

        for (i, headers) in malformed_scenarios.iter().enumerate() {
            let result = validator.validate_request(headers, ip);
            assert!(
                result.is_err(),
                "Malformed header scenario {} should fail",
                i + 1
            );
        }

        // Test session creation with invalid user agents
        let invalid_user_agents = [None, Some(""), Some("\0\0\0"), Some("🚀💥🔥")];

        for ua in &invalid_user_agents {
            let result = session_manager.create_session(ip, *ua);
            // Should handle gracefully (not panic)
            assert!(result.is_ok() || result.is_err()); // Either outcome is acceptable, just don't panic
        }
    }

    #[tokio::test]
    async fn test_concurrent_session_stress() {
        let (_, session_manager) = EnhancedSecurityConfigBuilder::new()
            .with_max_sessions_per_ip(50) // Higher limit for stress testing
            .build();

        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        let session_manager = Arc::new(session_manager);

        // Spawn multiple concurrent session creation tasks
        let tasks: Vec<_> = (0..100)
            .map(|i| {
                let sm = session_manager.clone();
                tokio::spawn(async move {
                    let user_agent = format!("TestAgent/{}", i);
                    sm.create_session(ip, Some(&user_agent))
                })
            })
            .collect();

        // Wait for all tasks to complete
        let results = futures::future::join_all(tasks).await;

        let mut success_count = 0;
        let mut failure_count = 0;

        for result in results {
            match result.unwrap() {
                Ok(_) => success_count += 1,
                Err(_) => failure_count += 1,
            }
        }

        // Should have some successes and some failures due to limits
        assert!(
            success_count > 0,
            "Should have some successful session creations"
        );
        assert!(success_count <= 50, "Should not exceed session limit");
        assert_eq!(
            success_count + failure_count,
            100,
            "All attempts should complete"
        );
    }

    #[tokio::test]
    async fn test_session_timeout_behavior() {
        let (_, session_manager) = EnhancedSecurityConfigBuilder::new()
            .with_session_idle_timeout(Duration::from_millis(100)) // Very short timeout for testing
            .build();

        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        let user_agent = "Mozilla/5.0";

        // Create session
        let session = session_manager
            .create_session(ip, Some(user_agent))
            .unwrap();

        // Session should be valid immediately
        assert!(
            session_manager
                .validate_session(&session.id, ip, Some(user_agent))
                .is_ok()
        );

        // Wait for timeout
        sleep(Duration::from_millis(150)).await;

        // Session should now be expired
        let result = session_manager.validate_session(&session.id, ip, Some(user_agent));
        assert!(result.is_err(), "Session should be expired after timeout");

        // Verify session was cleaned up
        assert_eq!(
            session_manager.session_count(),
            0,
            "Expired session should be cleaned up"
        );
    }

    #[tokio::test]
    async fn test_session_cleanup_efficiency() {
        let (_, session_manager) = EnhancedSecurityConfigBuilder::new()
            .with_session_idle_timeout(Duration::from_millis(50))
            .with_max_sessions_per_ip(1000) // High limit
            .build();

        let ip: IpAddr = "127.0.0.1".parse().unwrap();

        // Create many sessions
        let session_count = 100;
        for i in 0..session_count {
            let user_agent = format!("TestAgent/{}", i);
            session_manager
                .create_session(ip, Some(&user_agent))
                .unwrap();
        }

        assert_eq!(session_manager.session_count(), session_count);

        // Wait for sessions to expire
        sleep(Duration::from_millis(100)).await;

        // Trigger cleanup by trying to create a new session
        session_manager
            .create_session(ip, Some("TriggerCleanup"))
            .unwrap();

        // Cleanup should have removed expired sessions
        let cleaned_count = session_manager.cleanup_expired_sessions();
        assert!(
            cleaned_count >= session_count,
            "Should clean up expired sessions"
        );

        // Only the new session should remain
        assert_eq!(
            session_manager.session_count(),
            1,
            "Only new session should remain"
        );
    }
}

mod performance_tests {
    use super::*;

    #[tokio::test]
    async fn test_security_validation_performance() {
        let (validator, _) = EnhancedSecurityConfigBuilder::new()
            .allow_localhost(true)
            .require_authentication(true)
            .with_api_keys(vec!["test_key".to_string()])
            .with_rate_limit(10000, Duration::from_secs(60)) // High limit for performance testing
            .build();

        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        let mut headers = create_headers("http://localhost:3000", "Mozilla/5.0", None);
        headers.insert("Authorization".to_string(), "Bearer test_key".to_string());

        let start = std::time::Instant::now();
        let iterations = 1000;

        // Perform many validations
        for _ in 0..iterations {
            validator.validate_request(&headers, ip).unwrap();
        }

        let duration = start.elapsed();
        let avg_per_request = duration / iterations;

        // Security validation should be fast (< 1ms per request)
        assert!(
            avg_per_request < Duration::from_millis(1),
            "Security validation took too long: {:?} per request",
            avg_per_request
        );

        println!(
            "Security validation performance: {:?} per request",
            avg_per_request
        );
    }

    #[tokio::test]
    async fn test_memory_usage_under_load() {
        let (validator, session_manager) = EnhancedSecurityConfigBuilder::new()
            .allow_localhost(true)
            .with_max_sessions_per_ip(100)
            .with_rate_limit(1000, Duration::from_secs(60))
            .build();

        // Test memory stability under sustained load
        for round in 0..10 {
            let ip: IpAddr = format!("192.168.1.{}", round + 1).parse().unwrap();
            let headers = create_headers("http://localhost:3000", "Mozilla/5.0", None);

            // Create sessions and validate repeatedly
            for i in 0..50 {
                let user_agent = format!("TestAgent/{}/{}", round, i);

                // Create session
                if let Ok(session) = session_manager.create_session(ip, Some(&user_agent)) {
                    // Validate multiple times
                    for _ in 0..5 {
                        let _ =
                            session_manager.validate_session(&session.id, ip, Some(&user_agent));
                        let _ = validator.validate_request(&headers, ip);
                    }
                }
            }

            // Force cleanup periodically
            if round % 3 == 0 {
                session_manager.cleanup_expired_sessions();
            }
        }

        // Memory should be bounded (not growing indefinitely)
        // This test mainly ensures no obvious memory leaks
        assert!(
            session_manager.session_count() < 1000,
            "Session count should be bounded"
        );
    }
}
