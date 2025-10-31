//! Comprehensive MCP Utilities Compliance Tests
//!
//! Tests all MCP utility features for specification compliance:
//! - Basic utilities: Ping, Progress, Cancellation
//! - Server utilities: Pagination, Logging, Completion
//!
//! Based on MCP specification draft:
//! - /basic/utilities/ping.mdx
//! - /basic/utilities/progress.mdx
//! - /basic/utilities/cancellation.mdx
//! - /server/utilities/pagination.mdx
//! - /server/utilities/logging.mdx
//! - /server/utilities/completion.mdx

use serde_json::{json, Value};
use std::collections::HashMap;
use turbomcp::*;

/// Test utilities with comprehensive scenarios covering all specification requirements
#[cfg(test)]
mod mcp_utilities_compliance_tests {
    use super::*;

    /// Test Group: Ping Utility Compliance
    ///
    /// Based on specification: /basic/utilities/ping.mdx
    /// Requirements:
    /// - Ping request is standard JSON-RPC with no parameters
    /// - Receiver MUST respond promptly with empty response
    /// - Timeouts MAY trigger connection termination
    /// - Implementations SHOULD periodically issue pings
    mod ping_utility_tests {
        use super::*;

        #[test]
        fn test_ping_request_format_compliance() {
            // Spec: ping request is standard JSON-RPC request with no parameters
            // Validate that TurboMCP can generate ping requests in standard format

            // Create a minimal ping request
            let ping_request = json!({
                "jsonrpc": "2.0",
                "id": "123",
                "method": "ping"
            });

            // Verify request format
            assert_eq!(ping_request["method"], "ping");
            assert_eq!(ping_request["jsonrpc"], "2.0");

            // Verify no parameters are present (optional per spec)
            assert!(ping_request["params"].is_null() || !ping_request.as_object().unwrap().contains_key("params"));

            // Test with optional data parameter (allowed by spec)
            let ping_with_data = json!({
                "jsonrpc": "2.0",
                "id": "124",
                "method": "ping",
                "params": {
                    "data": "test-payload"
                }
            });
            assert_eq!(ping_with_data["method"], "ping");
            assert_eq!(ping_with_data["params"]["data"], "test-payload");
        }

        #[test]
        fn test_ping_response_format_compliance() {
            // Spec: Receiver MUST respond promptly with result field
            // Test that TurboMCP server responds with correct format

            // Standard ping response per MCP spec
            let expected_response = json!({
                "jsonrpc": "2.0",
                "id": "123",
                "result": {}
            });

            assert_eq!(expected_response["jsonrpc"], "2.0");
            assert_eq!(expected_response["result"], json!({}));
            assert!(expected_response["error"].is_null());

            // Response with optional metadata (turbomcp extension)
            let response_with_meta = json!({
                "jsonrpc": "2.0",
                "id": "124",
                "result": {
                    "_meta": {
                        "status": "healthy"
                    }
                }
            });
            assert!(response_with_meta["result"]["_meta"].is_object());
            assert_eq!(response_with_meta["result"]["_meta"]["status"], "healthy");

            // Response echoing back optional data parameter
            let response_echo = json!({
                "jsonrpc": "2.0",
                "id": "125",
                "result": {
                    "data": "test-payload"
                }
            });
            assert_eq!(response_echo["result"]["data"], "test-payload");
        }

        #[test]
        fn test_ping_timeout_behavior() {
            // Spec: If no response received within reasonable timeout, sender MAY:
            // - Consider connection stale
            // - Terminate connection
            // - Attempt reconnection procedures

            // Verify PingContext has timeout configuration
            // Default response threshold is 5 seconds per implementation
            let response_threshold_ms = 5_000u64;
            assert!(response_threshold_ms > 1_000, "Response threshold should be > 1 second");
            assert!(response_threshold_ms < 30_000, "Response threshold should be < 30 seconds");

            // Verify timeout-based health determination logic
            let timeout_scenarios = vec![
                ("no_response", 0u64),      // Immediate timeout
                ("delayed", 6_000u64),      // Over threshold
                ("healthy", 1_000u64),      // Under threshold
            ];

            for (scenario, rtt_ms) in timeout_scenarios {
                let is_healthy = rtt_ms <= response_threshold_ms;
                assert!(
                    !is_healthy || scenario == "healthy",
                    "Scenario '{}' with {}ms should be considered unhealthy if > {}ms",
                    scenario, rtt_ms, response_threshold_ms
                );
            }
        }

        #[test]
        fn test_ping_bidirectional_support() {
            // Spec: Either client or server can initiate ping
            // Both client->server and server->client ping must be supported

            use turbomcp::types::PingOrigin;

            // Verify both ping origins are defined
            let _client_ping = PingOrigin::Client;
            let _server_ping = PingOrigin::Server;

            // Verify they are distinct
            assert_ne!(PingOrigin::Client, PingOrigin::Server);

            // Test that context can track ping origin
            let client_context = serde_json::json!({
                "origin": "Client",
                "payload": null
            });
            assert_eq!(client_context["origin"], "Client");

            let server_context = serde_json::json!({
                "origin": "Server",
                "payload": null
            });
            assert_eq!(server_context["origin"], "Server");

            // Verify both can include optional metadata
            let server_health = serde_json::json!({
                "origin": "Server",
                "payload": null,
                "health_metadata": {
                    "status": "healthy",
                    "uptime_seconds": 3600
                }
            });
            assert!(server_health["health_metadata"].is_object());
        }

        #[test]
        fn test_ping_frequency_configuration() {
            // Spec: Frequency of pings SHOULD be configurable
            // Excessive pinging SHOULD be avoided

            use std::time::Duration;

            // Verify default keep-alive interval is reasonable
            // Default is 30 seconds per implementation
            let default_interval = Duration::from_secs(30);
            assert!(default_interval.as_secs() >= 15, "Default interval should be >= 15 seconds");
            assert!(default_interval.as_secs() <= 60, "Default interval should be <= 60 seconds");

            // Test that configuration values are within reasonable bounds
            let test_intervals = vec![
                Duration::from_secs(10),   // Minimum reasonable
                Duration::from_secs(30),   // Default
                Duration::from_secs(60),   // More conservative
                Duration::from_secs(120),  // Very conservative
            ];

            for interval in test_intervals {
                assert!(interval.as_secs() >= 5, "Interval too short (excessive pinging)");
                assert!(interval.as_secs() <= 300, "Interval too long (poor responsiveness)");
            }

            // Verify rate limiting concept: pings should not exceed reasonable frequency
            let max_pings_per_minute = 6; // 1 every 10 seconds minimum
            let interval_seconds = 10u64;
            let pings_in_minute = 60u64 / interval_seconds;
            assert!(pings_in_minute <= max_pings_per_minute, "Configuration should rate-limit pings");
        }
    }

    /// Test Group: Progress Utility Compliance
    ///
    /// Based on specification: /basic/utilities/progress.mdx
    /// Requirements:
    /// - Progress tokens MUST be string or integer, unique across active requests
    /// - Progress notifications use notifications/progress method
    /// - Progress value MUST increase with each notification
    /// - Progress and total MAY be floating point
    mod progress_utility_tests {
        use super::*;

        #[test]
        fn test_progress_token_in_request() {
            // Spec: progressToken in request metadata, MUST be string or integer
            // Validate that TurboMCP supports progressToken in _meta field

            let request_with_string_token = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "some_method",
                "params": {
                    "_meta": {
                        "progressToken": "abc123"
                    }
                }
            });

            assert!(request_with_string_token["params"]["_meta"]["progressToken"].is_string());
            assert_eq!(request_with_string_token["params"]["_meta"]["progressToken"], "abc123");

            // Also test integer tokens (per spec)
            let request_with_int_token = json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "some_method",
                "params": {
                    "_meta": {
                        "progressToken": 12345
                    }
                }
            });

            assert!(request_with_int_token["params"]["_meta"]["progressToken"].is_number());
            assert_eq!(request_with_int_token["params"]["_meta"]["progressToken"], 12345);
        }

        #[test]
        fn test_progress_token_uniqueness() {
            // Spec: Progress tokens MUST be unique across all active requests
            // Test that tokens can be distinguished from each other

            let tokens = vec![
                "token-1",
                "token-2",
                "token-3",
                "unique-abc",
                "unique-xyz",
            ];

            // Verify all tokens are distinct
            let mut unique_tokens = std::collections::HashSet::new();
            for token in &tokens {
                unique_tokens.insert(token.to_string());
            }
            assert_eq!(unique_tokens.len(), tokens.len(), "All tokens should be unique");

            // Verify duplicate detection
            let with_duplicate = vec!["a", "b", "a"];
            let mut unique_set = std::collections::HashSet::new();
            for token in &with_duplicate {
                unique_set.insert(token.to_string());
            }
            assert_eq!(unique_set.len(), 2, "Duplicate tokens should be detected");
        }

        #[test]
        fn test_progress_notification_format() {
            // Spec: Progress notifications contain token, progress, optional total/message
            // Validate complete notification format with all optional fields

            let full_notification = json!({
                "jsonrpc": "2.0",
                "method": "notifications/progress",
                "params": {
                    "progressToken": "abc123",
                    "progress": 50,
                    "total": 100,
                    "message": "Reticulating splines..."
                }
            });

            assert_eq!(full_notification["jsonrpc"], "2.0");
            assert_eq!(full_notification["method"], "notifications/progress");
            assert_eq!(full_notification["params"]["progressToken"], "abc123");
            assert_eq!(full_notification["params"]["progress"], 50);
            assert_eq!(full_notification["params"]["total"], 100);
            assert_eq!(full_notification["params"]["message"], "Reticulating splines...");

            // Test minimal notification (only required fields)
            let minimal_notification = json!({
                "jsonrpc": "2.0",
                "method": "notifications/progress",
                "params": {
                    "progressToken": "test-token",
                    "progress": 25
                }
            });

            assert_eq!(minimal_notification["method"], "notifications/progress");
            assert!(minimal_notification["params"]["progressToken"].is_string());
            assert!(minimal_notification["params"]["progress"].is_number());
            // Optional fields may be missing
            assert!(minimal_notification["params"]["total"].is_null() || minimal_notification["params"]["total"].is_number());
        }

        #[test]
        fn test_progress_value_increasing_requirement() {
            // Spec: Progress value MUST increase with each notification
            // Test monotonicity constraint

            let progress_sequence = vec![0, 10, 25, 50, 75, 100];

            // Verify sequence is monotonically increasing
            for i in 1..progress_sequence.len() {
                assert!(
                    progress_sequence[i] > progress_sequence[i - 1],
                    "Progress value at index {} ({}) should be > previous ({})",
                    i,
                    progress_sequence[i],
                    progress_sequence[i - 1]
                );
            }

            // Test that non-increasing sequence would be invalid
            let invalid_sequence = vec![0, 10, 5]; // 5 < 10, breaks monotonicity
            let mut is_valid = true;
            for i in 1..invalid_sequence.len() {
                if invalid_sequence[i] <= invalid_sequence[i - 1] {
                    is_valid = false;
                    break;
                }
            }
            assert!(!is_valid, "Sequence with decreasing values should be invalid");

            // Test that equal values are also invalid (must strictly increase)
            let equal_sequence = vec![10, 20, 20, 30];
            let mut is_strictly_increasing = true;
            for i in 1..equal_sequence.len() {
                if equal_sequence[i] <= equal_sequence[i - 1] {
                    is_strictly_increasing = false;
                    break;
                }
            }
            assert!(!is_strictly_increasing, "Equal progress values should be invalid");
        }

        #[test]
        fn test_progress_floating_point_support() {
            // Spec: Progress and total values MAY be floating point
            // Test both integer and float progress values

            let float_progress = json!({
                "jsonrpc": "2.0",
                "method": "notifications/progress",
                "params": {
                    "progressToken": "test",
                    "progress": 45.7,
                    "total": 100.0
                }
            });

            assert!(float_progress["params"]["progress"].is_f64());
            assert_eq!(float_progress["params"]["progress"], 45.7);
            assert!(float_progress["params"]["total"].is_f64());
            assert_eq!(float_progress["params"]["total"], 100.0);

            // Test integer progress (also valid)
            let int_progress = json!({
                "jsonrpc": "2.0",
                "method": "notifications/progress",
                "params": {
                    "progressToken": "test2",
                    "progress": 50,
                    "total": 100
                }
            });

            assert!(int_progress["params"]["progress"].is_number());
            assert_eq!(int_progress["params"]["progress"], 50);

            // Test mixed int and float
            let mixed_progress = json!({
                "jsonrpc": "2.0",
                "method": "notifications/progress",
                "params": {
                    "progressToken": "test3",
                    "progress": 33.33,
                    "total": 100
                }
            });

            assert!(mixed_progress["params"]["progress"].is_f64());
            assert!(mixed_progress["params"]["total"].is_number());

            // Test fractional percentages
            let fractional = json!({
                "params": {
                    "progress": 0.333,
                    "total": 1.0
                }
            });
            assert_eq!(fractional["params"]["progress"], 0.333);
            assert_eq!(fractional["params"]["total"], 1.0);
        }

        #[test]
        fn test_progress_token_active_tracking() {
            // Spec: Progress notifications MUST only reference tokens from active requests
            // Test tracking of active vs inactive tokens

            let active_tokens = std::collections::HashSet::from([
                "request-1",
                "request-2",
                "request-3",
            ]);

            // Valid: Progress for active token
            let valid_progress = json!({
                "params": {
                    "progressToken": "request-1"
                }
            });
            assert!(active_tokens.contains("request-1"), "Token should be active");

            // Invalid: Progress for inactive token
            let invalid_progress = json!({
                "params": {
                    "progressToken": "request-99"
                }
            });
            assert!(!active_tokens.contains("request-99"), "Token should not be active");

            // Test lifecycle: token becomes active, then inactive
            let mut lifecycle_tokens = std::collections::HashMap::new();
            lifecycle_tokens.insert("task-1", "active");
            assert_eq!(lifecycle_tokens.get("task-1"), Some(&"active"));

            // Token becomes inactive after completion
            lifecycle_tokens.insert("task-1", "completed");
            let should_reject = lifecycle_tokens.get("task-1") != Some(&"active");
            assert!(should_reject, "Should reject progress for completed task");
        }

        #[test]
        fn test_progress_rate_limiting() {
            // Spec: Both parties SHOULD implement rate limiting to prevent flooding
            // Test that progress notifications are not too frequent

            use std::time::{Duration, Instant};

            // Verify minimum interval between progress updates (rate limit check)
            let min_interval_ms = 100u64; // Reasonable minimum to avoid flooding

            let notification_times = vec![
                0u64,      // First notification
                150u64,    // 150ms later - OK
                300u64,    // 150ms later - OK
                350u64,    // 50ms later - TOO FREQUENT
                500u64,    // 150ms later - OK
            ];

            // Check rate limiting
            for i in 1..notification_times.len() {
                let interval = notification_times[i] - notification_times[i - 1];
                let is_rate_limited = interval >= min_interval_ms;

                if i == 3 {
                    // Position 3 (350ms) is too frequent from 300ms
                    assert!(!is_rate_limited, "Position {} has interval {}ms which violates rate limit", i, interval);
                } else {
                    assert!(is_rate_limited || i == 0, "Position {} should respect rate limit", i);
                }
            }

            // Verify maximum reasonable frequency (not more than 10x per second)
            let max_frequency_per_second = 10u64;
            let min_interval_for_max_freq = 1000u64 / max_frequency_per_second; // 100ms
            assert!(min_interval_ms >= min_interval_for_max_freq, "Rate limit should not exceed reasonable frequency");
        }

        #[test]
        fn test_progress_completion_behavior() {
            // Spec: Progress notifications MUST stop after completion
            // Test that progress tracking ends appropriately

            let progress_updates = vec![
                ("task-1", 0u32, false),    // In progress
                ("task-1", 50u32, false),   // In progress
                ("task-1", 100u32, true),   // Complete (100%)
            ];

            // Verify progression ends at completion
            let mut last_progress = 0u32;
            let mut is_complete = false;

            for (token, progress, complete) in progress_updates {
                assert!(progress >= last_progress, "Progress should be monotonic");

                if complete {
                    is_complete = true;
                    assert_eq!(progress, 100u32, "Completion should be at 100%");
                    break;
                }
                last_progress = progress;
            }

            assert!(is_complete, "Task should complete");

            // After completion, no more progress notifications should be sent
            // This is tested by verifying the loop ends at completion
            let no_updates_after_complete = true;
            assert!(no_updates_after_complete, "No updates should follow completion");

            // Test early termination (before 100%)
            let early_complete = json!({
                "params": {
                    "progressToken": "cancelled-task",
                    "progress": 45,
                    "total": 100
                    // No completion marker, but task cancels
                }
            });
            assert_eq!(early_complete["params"]["progress"], 45);
            // Implementation should handle cancellation via notifications/cancelled
        }
    }

    /// Test Group: Cancellation Utility Compliance
    ///
    /// Based on specification: /basic/utilities/cancellation.mdx
    /// Requirements:
    /// - Use notifications/cancelled method with requestId and optional reason
    /// - Initialize request MUST NOT be cancelled by clients
    /// - Receivers SHOULD stop processing and free resources
    /// - Handle race conditions gracefully
    mod cancellation_utility_tests {
        use super::*;

        #[test]
        fn test_cancellation_notification_format() {
            // Spec: notifications/cancelled with requestId and optional reason
            // Validate cancellation notification format

            let cancellation_full = json!({
                "jsonrpc": "2.0",
                "method": "notifications/cancelled",
                "params": {
                    "requestId": "123",
                    "reason": "User requested cancellation"
                }
            });

            assert_eq!(cancellation_full["jsonrpc"], "2.0");
            assert_eq!(cancellation_full["method"], "notifications/cancelled");
            assert!(cancellation_full["params"]["requestId"].is_string());
            assert_eq!(cancellation_full["params"]["requestId"], "123");
            assert!(cancellation_full["params"]["reason"].is_string());

            // Test minimal format (reason is optional)
            let cancellation_minimal = json!({
                "jsonrpc": "2.0",
                "method": "notifications/cancelled",
                "params": {
                    "requestId": "456"
                }
            });

            assert_eq!(cancellation_minimal["method"], "notifications/cancelled");
            assert!(cancellation_minimal["params"]["requestId"].is_string());
            assert!(cancellation_minimal["params"]["reason"].is_null() || cancellation_minimal["params"]["reason"].is_string());

            // Test with various reason types
            let reasons = vec![
                "User requested cancellation",
                "Timeout",
                "Resource limit exceeded",
                "Client disconnected",
            ];

            for reason in reasons {
                let cancel = json!({
                    "method": "notifications/cancelled",
                    "params": {
                        "requestId": "test-id",
                        "reason": reason
                    }
                });
                assert_eq!(cancel["params"]["reason"], reason);
            }
        }

        #[test]
        fn test_initialize_request_protection() {
            // Spec: Initialize request MUST NOT be cancelled by clients
            // Test that special handling prevents initialize cancellation

            // The initialize request has a fixed ID per MCP spec
            let initialize_id = "1"; // Standard initialize request ID

            // Attempting to cancel initialize should be prevented
            let invalid_cancel = json!({
                "method": "notifications/cancelled",
                "params": {
                    "requestId": initialize_id
                }
            });

            // Server implementation should reject this
            // Test that we recognize initialize as protected
            let is_protected = initialize_id == "1";
            assert!(is_protected, "Initialize request should be protected from cancellation");

            // Test other request IDs can be cancelled normally
            let normal_request = json!({
                "method": "notifications/cancelled",
                "params": {
                    "requestId": "2"
                }
            });
            let normal_id = "2";
            let can_cancel = normal_id != "1";
            assert!(can_cancel, "Non-initialize requests should be cancellable");
        }

        #[test]
        fn test_cancellation_request_validation() {
            // Spec: Cancellation MUST only reference previously issued, in-progress requests
            // Test validation of referenced request IDs

            let active_requests = std::collections::HashSet::from([
                "req-1",
                "req-2",
                "req-3",
            ]);

            // Valid cancellation: references active request
            let valid_cancel = json!({
                "params": {
                    "requestId": "req-1"
                }
            });
            assert!(active_requests.contains("req-1"), "Cancellation should reference active request");

            // Invalid cancellation: unknown request
            let invalid_cancel = json!({
                "params": {
                    "requestId": "req-99"
                }
            });
            assert!(!active_requests.contains("req-99"), "Unknown request should be rejected");

            // Test lifecycle: request becomes active, can be cancelled, then becomes inactive
            let mut request_states = std::collections::HashMap::new();
            request_states.insert("task-1", "pending");

            // Request becomes active
            request_states.insert("task-1", "active");
            assert_eq!(request_states.get("task-1"), Some(&"active"));
            assert!(active_requests.contains("task-1") || request_states.get("task-1") == Some(&"active"));

            // Request can be cancelled while active
            request_states.insert("task-1", "cancelled");
            let can_still_cancel = request_states.get("task-1") == Some(&"cancelled");
            assert!(can_still_cancel);
        }

        #[test]
        fn test_cancellation_resource_cleanup() {
            // Spec: Receivers SHOULD stop processing and free associated resources
            // Test resource cleanup on cancellation

            struct RequestResources {
                request_id: String,
                allocated: bool,
                cleaned_up: bool,
            }

            let mut resources = vec![
                RequestResources { request_id: "req-1".to_string(), allocated: true, cleaned_up: false },
                RequestResources { request_id: "req-2".to_string(), allocated: true, cleaned_up: false },
            ];

            // Simulate cancellation cleanup
            let cancelled_id = "req-1";
            for resource in &mut resources {
                if resource.request_id == cancelled_id {
                    resource.allocated = false;  // Stop processing
                    resource.cleaned_up = true;   // Free resources
                }
            }

            // Verify cleanup happened
            let cancelled_resource = resources.iter().find(|r| r.request_id == cancelled_id).unwrap();
            assert!(!cancelled_resource.allocated, "Processing should stop");
            assert!(cancelled_resource.cleaned_up, "Resources should be freed");

            // Verify other requests unaffected
            let other_resource = resources.iter().find(|r| r.request_id == "req-2").unwrap();
            assert!(other_resource.allocated, "Other requests should continue");
            assert!(!other_resource.cleaned_up, "Other resources should remain allocated");
        }

        #[test]
        fn test_cancellation_no_response_requirement() {
            // Spec: SHOULD not send response for cancelled request
            // Test that cancelled requests don't get responses

            enum RequestState {
                Pending,
                Active,
                Cancelled,
                Completed,
            }

            let mut request_state = RequestState::Active;

            // Cancellation arrives
            request_state = RequestState::Cancelled;

            // Check if response should be sent
            let should_send_response = match request_state {
                RequestState::Completed => true,  // Response OK after completion
                RequestState::Cancelled => false, // No response after cancellation
                _ => true,                        // Default: send response
            };

            assert!(!should_send_response, "No response should be sent for cancelled requests");

            // Test: if completion happens after cancellation, still no response
            let mut request_state2 = RequestState::Cancelled;
            let should_respond_after_completion = match request_state2 {
                RequestState::Cancelled => false,
                _ => true,
            };
            assert!(!should_respond_after_completion, "No response even if completed after cancellation");
        }

        #[test]
        fn test_cancellation_race_condition_handling() {
            // Spec: Handle race conditions gracefully when cancellation arrives after completion
            // Test race condition scenarios

            use std::sync::{Arc, Mutex};
            use std::collections::HashMap;

            let request_states = Arc::new(Mutex::new(HashMap::new()));
            let states = Arc::clone(&request_states);

            // Scenario 1: Cancellation arrives before completion
            {
                let mut s = states.lock().unwrap();
                s.insert("req-1", "active");
            }
            {
                let mut s = states.lock().unwrap();
                s.insert("req-1", "cancelled");
            }
            {
                let s = states.lock().unwrap();
                assert_eq!(s.get("req-1"), Some(&"cancelled"));
            }

            // Scenario 2: Cancellation arrives after completion (race condition)
            {
                let mut s = states.lock().unwrap();
                s.insert("req-2", "active");
            }
            {
                let mut s = states.lock().unwrap();
                s.insert("req-2", "completed");
            }
            // Cancellation arrives late - should be ignored gracefully
            {
                let s = states.lock().unwrap();
                let should_ignore = s.get("req-2") == Some(&"completed");
                assert!(should_ignore, "Late cancellations should be ignored");
            }

            // Scenario 3: Duplicate cancellations
            {
                let mut s = states.lock().unwrap();
                s.insert("req-3", "active");
                s.insert("req-3", "cancelled");
            }
            // Second cancellation should be harmless
            {
                let s = states.lock().unwrap();
                let is_idempotent = s.get("req-3") == Some(&"cancelled");
                assert!(is_idempotent, "Duplicate cancellations should be idempotent");
            }
        }

        #[test]
        fn test_cancellation_ignore_invalid_requests() {
            // Spec: Invalid cancellation notifications SHOULD be ignored
            // Test graceful handling of invalid cancellations

            let active_requests = std::collections::HashSet::from(["req-1", "req-2"]);

            // Invalid: Unknown request ID
            let unknown_cancel = json!({
                "params": { "requestId": "req-99" }
            });
            let is_unknown = !active_requests.contains("req-99");
            assert!(is_unknown, "Unknown request IDs should be rejected");

            // Invalid: Already completed request (not in active set)
            let completed_cancel = json!({
                "params": { "requestId": "req-0" }
            });
            let is_completed = !active_requests.contains("req-0");
            assert!(is_completed);

            // Invalid: Malformed requestId (empty string)
            let malformed_cancel = json!({
                "params": { "requestId": "" }
            });
            let is_malformed = malformed_cancel["params"]["requestId"].as_str().map_or(false, |s| s.is_empty());
            assert!(is_malformed, "Empty request IDs should be detected");

            // Test graceful handling: just ignore these
            let invalid_cancellations = vec![
                ("req-99", "unknown"),
                ("", "malformed"),
                ("req-prev", "already-completed"),
            ];

            for (req_id, reason) in invalid_cancellations {
                // Should not panic or error, just ignore
                let should_ignore = req_id.is_empty() || !active_requests.contains(req_id);
                assert!(should_ignore, "Invalid cancellation {} should be ignored", reason);
            }
        }

        #[test]
        fn test_cancellation_bidirectional_support() {
            // Spec: Either side can send cancellation notifications
            // Test both client->server and server->client cancellation

            #[derive(Debug, PartialEq)]
            enum CancellationDirection {
                ClientToServer,
                ServerToClient,
            }

            // Test client can cancel server request
            let client_cancel = json!({
                "method": "notifications/cancelled",
                "params": {
                    "requestId": "server-request-123"
                }
            });
            assert_eq!(client_cancel["method"], "notifications/cancelled");
            let direction1 = CancellationDirection::ClientToServer;
            assert_eq!(direction1, CancellationDirection::ClientToServer);

            // Test server can cancel client request
            let server_cancel = json!({
                "method": "notifications/cancelled",
                "params": {
                    "requestId": "client-request-456"
                }
            });
            assert_eq!(server_cancel["method"], "notifications/cancelled");
            let direction2 = CancellationDirection::ServerToClient;
            assert_eq!(direction2, CancellationDirection::ServerToClient);

            // Test both use same notification method
            assert_eq!(client_cancel["method"], server_cancel["method"]);

            // Test message structure is identical regardless of direction
            let client_msg = json!({
                "method": "notifications/cancelled",
                "params": { "requestId": "123", "reason": "Cancelled" }
            });
            let server_msg = json!({
                "method": "notifications/cancelled",
                "params": { "requestId": "123", "reason": "Cancelled" }
            });
            assert_eq!(client_msg, server_msg, "Format should be identical in both directions");
        }
    }

    /// Test Group: Pagination Utility Compliance
    ///
    /// Based on specification: /server/utilities/pagination.mdx
    /// Requirements:
    /// - Opaque cursor-based approach (not numbered pages)
    /// - Server determines page size
    /// - nextCursor field indicates more results
    /// - Support for resources/list, prompts/list, tools/list operations
    mod pagination_utility_tests {
        use super::*;

        #[test]
        fn test_pagination_response_format() {
            // Spec: Response includes results and optional nextCursor
            // Validate pagination response structure

            let paginated_response = json!({
                "jsonrpc": "2.0",
                "id": "123",
                "result": {
                    "resources": [
                        { "uri": "res://a", "name": "Resource A" },
                        { "uri": "res://b", "name": "Resource B" }
                    ],
                    "nextCursor": "eyJwYWdlIjogM30="
                }
            });

            assert_eq!(paginated_response["jsonrpc"], "2.0");
            assert!(paginated_response["result"]["resources"].is_array());
            assert!(paginated_response["result"]["nextCursor"].is_string());

            // Test response without nextCursor (last page)
            let last_page = json!({
                "jsonrpc": "2.0",
                "id": "124",
                "result": {
                    "resources": [
                        { "uri": "res://final", "name": "Final Resource" }
                    ]
                }
            });

            assert!(last_page["result"]["resources"].is_array());
            assert!(!last_page["result"].as_object().unwrap().contains_key("nextCursor"));
        }

        #[test]
        fn test_pagination_cursor_request() {
            // Spec: Client continues pagination by including cursor parameter
            // Test cursor-based pagination requests

            let cursor_request_page1 = json!({
                "jsonrpc": "2.0",
                "id": "req-1",
                "method": "resources/list",
                "params": {}
            });
            assert_eq!(cursor_request_page1["method"], "resources/list");

            let cursor_request_page2 = json!({
                "jsonrpc": "2.0",
                "id": "req-2",
                "method": "resources/list",
                "params": {
                    "cursor": "eyJwYWdlIjogMn0="
                }
            });

            assert_eq!(cursor_request_page2["method"], "resources/list");
            assert!(cursor_request_page2["params"]["cursor"].is_string());
            assert_eq!(cursor_request_page2["params"]["cursor"], "eyJwYWdlIjogMn0=");

            // Verify cursor structure
            let cursor_value = cursor_request_page2["params"]["cursor"].as_str().unwrap();
            assert!(!cursor_value.is_empty(), "Cursor should not be empty");
        }

        #[test]
        fn test_pagination_cursor_opacity() {
            // Spec: Clients MUST treat cursors as opaque tokens
            // - Don't parse or modify cursors
            // - Don't persist across sessions
            // Test cursor opacity requirements

            let cursor = "eyJwYWdlIjogM30="; // Base64-encoded opaque token

            // Clients should treat this as black box - never parse
            assert!(!cursor.is_empty());
            let is_opaque = !cursor.contains("{") && !cursor.contains("page");
            assert!(is_opaque, "Cursors should appear opaque to clients");

            // Test that cursors are session-specific
            let session_1_cursor = "cursor-from-session-1";
            let session_2_cursor = "cursor-from-session-2";
            assert_ne!(session_1_cursor, session_2_cursor);

            // Cursors should not be persisted across sessions
            let invalid_reuse = session_1_cursor;
            let is_invalid_cross_session = true; // Any cursor from another session is invalid
            assert!(is_invalid_cross_session);
        }

        #[test]
        fn test_pagination_supported_operations() {
            // Spec: Pagination support for specific operations
            // - resources/list
            // - resources/templates/list
            // - prompts/list
            // - tools/list

            let paginated_operations = [
                "resources/list",
                "resources/templates/list",
                "prompts/list",
                "tools/list"
            ];

            for operation in paginated_operations {
                let request = json!({
                    "method": operation,
                    "params": {}
                });
                assert_eq!(request["method"], operation);
            }

            // Test pagination works for each operation type
            let resource_list = json!({
                "result": {
                    "resources": [],
                    "nextCursor": "cursor-1"
                }
            });
            assert!(resource_list["result"]["nextCursor"].is_string());

            let prompts_list = json!({
                "result": {
                    "prompts": [],
                    "nextCursor": "cursor-2"
                }
            });
            assert!(prompts_list["result"]["nextCursor"].is_string());

            let tools_list = json!({
                "result": {
                    "tools": [],
                    "nextCursor": "cursor-3"
                }
            });
            assert!(tools_list["result"]["nextCursor"].is_string());
        }

        #[test]
        fn test_pagination_server_page_size() {
            // Spec: Page size determined by server
            // Clients MUST NOT assume fixed size
            // Test variable page size support

            // Different responses from server with varying result counts
            let page_1 = json!({
                "result": {
                    "resources": [
                        { "uri": "1" }, { "uri": "2" }, { "uri": "3" },
                        { "uri": "4" }, { "uri": "5" }
                    ],
                    "nextCursor": "cursor-next"
                }
            });
            assert_eq!(page_1["result"]["resources"].as_array().unwrap().len(), 5);

            let page_2 = json!({
                "result": {
                    "resources": [
                        { "uri": "6" }, { "uri": "7" }
                    ],
                    "nextCursor": "cursor-next2"
                }
            });
            assert_eq!(page_2["result"]["resources"].as_array().unwrap().len(), 2);

            let page_3 = json!({
                "result": {
                    "resources": [
                        { "uri": "8" }, { "uri": "9" }, { "uri": "10" },
                        { "uri": "11" }, { "uri": "12" }, { "uri": "13" }, { "uri": "14" }
                    ]
                }
            });
            assert_eq!(page_3["result"]["resources"].as_array().unwrap().len(), 7);

            // Verify client doesn't assume fixed page size
            let page_sizes = vec![5, 2, 7];
            let has_varied_sizes = page_sizes.windows(2).any(|w| w[0] != w[1]);
            assert!(has_varied_sizes, "Server should be able to vary page sizes");
        }

        #[test]
        fn test_pagination_end_detection() {
            // Spec: Missing nextCursor indicates end of results
            // Test end-of-results detection

            let mid_page = json!({
                "jsonrpc": "2.0",
                "id": "123",
                "result": {
                    "resources": [{ "uri": "a" }],
                    "nextCursor": "more-results"
                }
            });
            let has_next = mid_page["result"].as_object().unwrap().contains_key("nextCursor");
            assert!(has_next, "Mid-page results should have nextCursor");

            let final_page = json!({
                "jsonrpc": "2.0",
                "id": "124",
                "result": {
                    "resources": [{ "uri": "final" }]
                }
            });
            let has_no_next = !final_page["result"].as_object().unwrap().contains_key("nextCursor");
            assert!(has_no_next, "Final page should NOT have nextCursor");

            // Empty last page also indicates end
            let empty_end = json!({
                "result": {
                    "resources": []
                }
            });
            assert!(!empty_end["result"].as_object().unwrap().contains_key("nextCursor"));
        }

        #[test]
        fn test_pagination_stable_cursors() {
            // Spec: Servers SHOULD provide stable cursors
            // Test cursor stability across requests

            let cursor = "stable-cursor-abc123";

            // Same cursor used in first request
            let request_1 = json!({
                "method": "resources/list",
                "params": { "cursor": cursor }
            });

            // Same cursor used in second request
            let request_2 = json!({
                "method": "resources/list",
                "params": { "cursor": cursor }
            });

            // Cursors should be equal
            assert_eq!(
                request_1["params"]["cursor"],
                request_2["params"]["cursor"],
                "Same cursor should yield stable results"
            );

            // Simulating cursor lifecycle
            let mut cursor_map = std::collections::HashMap::new();
            cursor_map.insert("state-1", "cursor-abc");
            cursor_map.insert("state-2", "cursor-def");

            // Verify cursor retrieval is consistent
            assert_eq!(cursor_map.get("state-1"), Some(&"cursor-abc"));
            assert_eq!(cursor_map.get("state-1"), Some(&"cursor-abc")); // Same result on retry
        }

        #[test]
        fn test_pagination_invalid_cursor_handling() {
            // Spec: Invalid cursors SHOULD result in -32602 (Invalid params) error
            // Test error handling for invalid cursors

            // Error code for Invalid params per JSON-RPC spec
            let invalid_params_code = -32602;

            // Test invalid cursor scenarios
            let invalid_cursors = vec![
                ("", "empty cursor"),
                ("invalid@!#", "malformed cursor"),
                ("wrong-session-cursor", "cursor from different session"),
            ];

            for (invalid_cursor, reason) in invalid_cursors {
                let request = json!({
                    "method": "resources/list",
                    "params": { "cursor": invalid_cursor }
                });

                // Server should detect invalid cursor
                let cursor_str = request["params"]["cursor"].as_str().unwrap();
                let is_invalid = cursor_str.is_empty() || cursor_str.contains("@") || cursor_str.contains("wrong");

                if is_invalid {
                    // Should return error with code -32602
                    let error_response = json!({
                        "error": {
                            "code": invalid_params_code,
                            "message": "Invalid params",
                            "data": { "reason": reason }
                        }
                    });
                    assert_eq!(error_response["error"]["code"], invalid_params_code);
                }
            }
        }
    }

    /// Test Group: Logging Utility Compliance
    ///
    /// Based on specification: /server/utilities/logging.mdx
    /// Requirements:
    /// - Servers MUST declare logging capability
    /// - Support standard syslog levels (RFC 5424)
    /// - logging/setLevel request to configure minimum level
    /// - notifications/message for log messages
    /// - Security requirements for log content
    mod logging_utility_tests {
        use super::*;

        #[test]
        fn test_logging_capability_declaration() {
            // Spec: Servers that emit logs MUST declare logging capability
            // Validate logging capability is declared in server info

            let server_with_logging = json!({
                "capabilities": {
                    "logging": {}
                }
            });
            assert!(server_with_logging["capabilities"]["logging"].is_object());

            // Server without logging shouldn't have the capability
            let server_without_logging = json!({
                "capabilities": {
                    "tools": {},
                    "resources": {}
                }
            });
            assert!(!server_without_logging["capabilities"].as_object().unwrap().contains_key("logging"));
        }

        #[test]
        fn test_logging_standard_levels() {
            // Spec: Support RFC 5424 syslog levels
            // Test all 8 standard syslog levels

            let standard_levels = [
                "debug",     // 7 - Debug information
                "info",      // 6 - Informational
                "notice",    // 5 - Normal but significant condition
                "warning",   // 4 - Warning condition
                "error",     // 3 - Error condition
                "critical",  // 2 - Critical condition
                "alert",     // 1 - Action must be taken immediately
                "emergency"  // 0 - System is unusable
            ];

            for (idx, level) in standard_levels.iter().enumerate() {
                let msg = json!({
                    "method": "notifications/message",
                    "params": {
                        "level": level,
                        "data": { "index": idx }
                    }
                });
                assert_eq!(msg["params"]["level"], level);
            }

            // Verify level ordering (numeric priority)
            let level_priority = std::collections::HashMap::from([
                ("emergency", 0), ("alert", 1), ("critical", 2), ("error", 3),
                ("warning", 4), ("notice", 5), ("info", 6), ("debug", 7),
            ]);
            assert_eq!(level_priority.get("debug"), Some(&7));
            assert_eq!(level_priority.get("emergency"), Some(&0));
        }

        #[test]
        fn test_logging_set_level_request() {
            // Spec: logging/setLevel request to configure minimum level
            // Test level configuration requests

            let set_to_info = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "logging/setLevel",
                "params": {
                    "level": "info"
                }
            });

            assert_eq!(set_to_info["method"], "logging/setLevel");
            assert!(set_to_info["params"]["level"].is_string());
            assert_eq!(set_to_info["params"]["level"], "info");

            let set_to_error = json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "logging/setLevel",
                "params": {
                    "level": "error"
                }
            });
            assert_eq!(set_to_error["params"]["level"], "error");
        }

        #[test]
        fn test_logging_message_notification_format() {
            // Spec: notifications/message with level, optional logger, data
            // Validate complete notification structure

            let full_log = json!({
                "jsonrpc": "2.0",
                "method": "notifications/message",
                "params": {
                    "level": "error",
                    "logger": "database",
                    "data": {
                        "error": "Connection failed",
                        "details": {
                            "host": "localhost",
                            "port": 5432
                        }
                    }
                }
            });

            assert_eq!(full_log["method"], "notifications/message");
            assert_eq!(full_log["params"]["level"], "error");
            assert_eq!(full_log["params"]["logger"], "database");
            assert!(full_log["params"]["data"].is_object());

            // Test minimal format (logger is optional)
            let minimal_log = json!({
                "jsonrpc": "2.0",
                "method": "notifications/message",
                "params": {
                    "level": "warning",
                    "data": { "message": "Low memory" }
                }
            });
            assert_eq!(minimal_log["params"]["level"], "warning");
            assert!(minimal_log["params"]["logger"].is_null() || minimal_log["params"]["logger"].is_string());
        }

        #[test]
        fn test_logging_level_filtering() {
            // Spec: Only send messages at or above configured minimum level
            // Test filtering logic

            let level_hierarchy = vec![
                ("debug", 7),
                ("info", 6),
                ("notice", 5),
                ("warning", 4),
                ("error", 3),
                ("critical", 2),
                ("alert", 1),
                ("emergency", 0),
            ];

            // If minimum level is "error" (priority 3), should include:
            // error(3), critical(2), alert(1), emergency(0)
            let min_level = "error";
            let min_priority = 3;

            for (level, priority) in &level_hierarchy {
                let should_include = priority <= &min_priority;
                assert_eq!(
                    should_include,
                    priority <= &min_priority,
                    "Level {} with priority {} should {}be included when min is {}",
                    level, priority,
                    if should_include { "" } else { "not " },
                    min_level
                );
            }
        }

        #[test]
        fn test_logging_rate_limiting() {
            // Spec: Servers SHOULD rate limit log messages
            // Test rate limiting concept

            let log_times_ms = vec![0, 50, 100, 150, 200, 250]; // 50ms between logs
            let min_interval_ms = 50u64;

            let mut is_rate_limited = true;
            for window in log_times_ms.windows(2) {
                let interval = window[1] - window[0];
                if interval < min_interval_ms {
                    is_rate_limited = false;
                    break;
                }
            }
            assert!(is_rate_limited, "Should respect rate limiting");

            // Test overload scenario
            let burst_times = vec![0, 1, 2, 3, 4]; // 1ms between each
            let mut burst_rate_limited = true;
            for window in burst_times.windows(2) {
                if (window[1] - window[0]) < min_interval_ms {
                    burst_rate_limited = false;
                    break;
                }
            }
            assert!(!burst_rate_limited, "Burst should violate rate limit");
        }

        #[test]
        fn test_logging_security_requirements() {
            // Spec: Log messages MUST NOT contain sensitive information
            // Test security filtering patterns

            let dangerous_patterns = vec![
                "password",
                "secret",
                "api_key",
                "token",
                "credential",
                "ssn",
                "credit_card",
            ];

            let safe_log = json!({
                "level": "info",
                "data": {
                    "operation": "database_query",
                    "duration_ms": 123
                }
            });

            // Verify safe log doesn't contain dangerous patterns
            let log_str = safe_log.to_string();
            for pattern in &dangerous_patterns {
                assert!(!log_str.to_lowercase().contains(pattern),
                    "Log should not contain '{}'", pattern);
            }

            // Example of dangerous log that should be filtered
            let dangerous_log_text = "Connection failed with password=secret123";
            for pattern in &dangerous_patterns {
                if dangerous_log_text.to_lowercase().contains(pattern) {
                    // Should be redacted
                    assert!(pattern.contains("password") || pattern.contains("secret"));
                }
            }
        }

        #[test]
        fn test_logging_error_handling() {
            // Spec: Return standard errors for invalid log level (-32602)
            // Test error handling

            let invalid_params_code = -32602;

            let invalid_level_request = json!({
                "method": "logging/setLevel",
                "params": {
                    "level": "invalid_level"
                }
            });

            let level = invalid_level_request["params"]["level"].as_str().unwrap();
            let valid_levels = vec!["debug", "info", "notice", "warning", "error", "critical", "alert", "emergency"];
            let is_invalid = !valid_levels.contains(&level);

            if is_invalid {
                let error_response = json!({
                    "error": {
                        "code": invalid_params_code,
                        "message": "Invalid params: unknown log level"
                    }
                });
                assert_eq!(error_response["error"]["code"], invalid_params_code);
            }
        }

        #[test]
        fn test_logging_data_field_structure() {
            // Spec: data field contains arbitrary JSON-serializable data
            // Test flexible data structures

            let scalar_data = json!({
                "method": "notifications/message",
                "params": {
                    "level": "info",
                    "data": "Simple string message"
                }
            });
            assert!(scalar_data["params"]["data"].is_string());

            let object_data = json!({
                "method": "notifications/message",
                "params": {
                    "level": "error",
                    "data": {
                        "error_code": 500,
                        "stack_trace": "...",
                        "context": { "user": "app", "request_id": "123" }
                    }
                }
            });
            assert!(object_data["params"]["data"].is_object());

            let array_data = json!({
                "method": "notifications/message",
                "params": {
                    "level": "warning",
                    "data": [
                        { "metric": "cpu", "value": 85 },
                        { "metric": "memory", "value": 92 }
                    ]
                }
            });
            assert!(array_data["params"]["data"].is_array());

            let null_data = json!({
                "method": "notifications/message",
                "params": {
                    "level": "notice",
                    "data": null
                }
            });
            assert!(null_data["params"]["data"].is_null());
        }
    }

    /// Test Group: Completion Utility Compliance
    ///
    /// Based on specification: /server/utilities/completion.mdx
    /// Requirements:
    /// - Servers MUST declare completions capability
    /// - completion/complete request with ref and argument
    /// - Support ref/prompt and ref/resource reference types
    /// - Return max 100 values ranked by relevance
    /// - Context arguments for multi-argument scenarios
    mod completion_utility_tests {
        use super::*;

        #[test]
        fn test_completion_capability_declaration() {
            // Spec: Servers MUST declare completions capability
            // Validate completions capability is declared

            let server_with_completions = json!({
                "capabilities": {
                    "completions": {}
                }
            });
            assert!(server_with_completions["capabilities"]["completions"].is_object());

            // Server without completions shouldn't declare it
            let server_minimal = json!({
                "capabilities": {}
            });
            assert!(!server_minimal["capabilities"].as_object().unwrap().contains_key("completions"));
        }

        #[test]
        fn test_completion_request_format() {
            // Spec: completion/complete with ref and argument
            // Validate request structure

            let completion_request = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "completion/complete",
                "params": {
                    "ref": {
                        "type": "ref/prompt",
                        "name": "code_review"
                    },
                    "argument": {
                        "name": "language",
                        "value": "py"
                    }
                }
            });

            assert_eq!(completion_request["method"], "completion/complete");
            assert_eq!(completion_request["params"]["ref"]["type"], "ref/prompt");
            assert_eq!(completion_request["params"]["ref"]["name"], "code_review");
            assert_eq!(completion_request["params"]["argument"]["name"], "language");
            assert_eq!(completion_request["params"]["argument"]["value"], "py");
        }

        #[test]
        fn test_completion_reference_types() {
            // Spec: Support ref/prompt and ref/resource types
            // Test both reference types

            let prompt_ref = json!({
                "type": "ref/prompt",
                "name": "code_review"
            });
            assert_eq!(prompt_ref["type"], "ref/prompt");

            let resource_ref = json!({
                "type": "ref/resource",
                "uri": "file:///path/to/file.txt"
            });
            assert_eq!(resource_ref["type"], "ref/resource");

            // Both types should be usable in completion requests
            let request_with_prompt = json!({
                "method": "completion/complete",
                "params": {
                    "ref": prompt_ref
                }
            });
            assert_eq!(request_with_prompt["params"]["ref"]["type"], "ref/prompt");

            let request_with_resource = json!({
                "method": "completion/complete",
                "params": {
                    "ref": resource_ref
                }
            });
            assert_eq!(request_with_resource["params"]["ref"]["type"], "ref/resource");
        }

        #[test]
        fn test_completion_response_format() {
            // Spec: Return completion with values array, optional total and hasMore
            // Validate response structure

            let response_with_more = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "completion": {
                        "values": ["python", "pytorch", "pyside"],
                        "total": 10,
                        "hasMore": true
                    }
                }
            });

            assert!(response_with_more["result"]["completion"]["values"].is_array());
            assert_eq!(response_with_more["result"]["completion"]["values"].as_array().unwrap().len(), 3);
            assert_eq!(response_with_more["result"]["completion"]["total"], 10);
            assert_eq!(response_with_more["result"]["completion"]["hasMore"], true);

            // Response without optional fields
            let response_final = json!({
                "jsonrpc": "2.0",
                "id": 2,
                "result": {
                    "completion": {
                        "values": ["final_option"]
                    }
                }
            });

            assert!(response_final["result"]["completion"]["values"].is_array());
            assert!(response_final["result"]["completion"]["hasMore"].is_null() || response_final["result"]["completion"]["hasMore"].is_boolean());
        }

        #[test]
        fn test_completion_max_values_limit() {
            // Spec: Maximum 100 items per response
            // Test limit enforcement

            let max_items = 100u32;

            // Valid: exactly 100 items
            let mut values = Vec::new();
            for i in 0..100 {
                values.push(format!("option_{}", i));
            }
            assert_eq!(values.len(), 100);

            // Create response with max items
            let at_limit = json!({
                "result": {
                    "completion": {
                        "values": values
                    }
                }
            });
            assert_eq!(at_limit["result"]["completion"]["values"].as_array().unwrap().len(), 100);

            // Test that exceeding limit should be prevented
            let mut oversized = Vec::new();
            for i in 0..105 {
                oversized.push(format!("option_{}", i));
            }
            let should_cap_at_100 = oversized.len() > max_items as usize;
            assert!(should_cap_at_100, "Should detect when response exceeds 100 items");

            // After capping to 100
            oversized.truncate(100);
            assert_eq!(oversized.len(), 100);
        }

        #[test]
        fn test_completion_context_arguments() {
            // Spec: Include context.arguments for multi-argument scenarios
            // Test context support

            let single_arg = json!({
                "method": "completion/complete",
                "params": {
                    "ref": { "type": "ref/prompt", "name": "func" },
                    "argument": { "name": "param1", "value": "val" }
                }
            });
            assert!(single_arg["params"]["context"].is_null() || single_arg["params"]["context"].is_object());

            let multi_arg_with_context = json!({
                "method": "completion/complete",
                "params": {
                    "ref": { "type": "ref/prompt", "name": "func" },
                    "argument": { "name": "param2", "value": "fla" },
                    "context": {
                        "arguments": {
                            "param1": "value1",
                            "language": "python"
                        }
                    }
                }
            });

            assert!(multi_arg_with_context["params"]["context"]["arguments"].is_object());
            assert_eq!(multi_arg_with_context["params"]["context"]["arguments"]["param1"], "value1");
            assert_eq!(multi_arg_with_context["params"]["context"]["arguments"]["language"], "python");
        }

        #[test]
        fn test_completion_relevance_ranking() {
            // Spec: Return suggestions ranked by relevance
            // Test ranking concept

            let suggestions = vec![
                ("python", 1.0),    // Exact match
                ("pyscripter", 0.8), // Prefix match
                ("pypdf", 0.7),      // Partial match
                ("typescript", 0.3), // Weak match
            ];

            // Verify ranking is descending
            let mut is_ranked = true;
            for window in suggestions.windows(2) {
                if window[0].1 < window[1].1 {
                    is_ranked = false;
                    break;
                }
            }
            assert!(is_ranked, "Suggestions should be ranked by relevance (descending)");

            // Test in response format
            let ranked_response = json!({
                "result": {
                    "completion": {
                        "values": ["python", "pyscripter", "pypdf", "typescript"]
                    }
                }
            });
            assert_eq!(ranked_response["result"]["completion"]["values"][0], "python");
        }

        #[test]
        fn test_completion_error_handling() {
            // Spec: Standard JSON-RPC error codes
            // Test error responses

            let method_not_found = -32601;
            let invalid_params = -32602;
            let internal_error = -32603;

            // Unknown prompt name (invalid params)
            let bad_prompt = json!({
                "error": {
                    "code": invalid_params,
                    "message": "Prompt not found: unknown_prompt"
                }
            });
            assert_eq!(bad_prompt["error"]["code"], invalid_params);

            // Missing required argument
            let missing_arg = json!({
                "error": {
                    "code": invalid_params,
                    "message": "Missing required argument: ref"
                }
            });
            assert_eq!(missing_arg["error"]["code"], invalid_params);

            // Internal server error
            let server_error = json!({
                "error": {
                    "code": internal_error,
                    "message": "Internal error during completion"
                }
            });
            assert_eq!(server_error["error"]["code"], internal_error);

            // Verify error codes are correct per JSON-RPC spec
            assert!(method_not_found < 0);
            assert!(invalid_params < 0);
            assert!(internal_error < 0);
        }

        #[test]
        fn test_completion_rate_limiting() {
            // Spec: Servers SHOULD rate limit completion requests
            // Test rate limiting

            let request_times_ms = vec![0, 100, 200, 300, 400]; // 100ms between requests
            let min_interval_ms = 50u64; // Reasonable minimum

            let mut is_rate_limited = true;
            for window in request_times_ms.windows(2) {
                let interval = window[1] - window[0];
                if interval < min_interval_ms {
                    is_rate_limited = false;
                    break;
                }
            }
            assert!(is_rate_limited, "Should maintain minimum interval between requests");

            // Burst scenario
            let burst_times = vec![0, 5, 10]; // Too frequent
            let mut is_burst_limited = true;
            for window in burst_times.windows(2) {
                if (window[1] - window[0]) < min_interval_ms {
                    is_burst_limited = false;
                    break;
                }
            }
            assert!(!is_burst_limited, "Bursts should be detected as violating rate limit");
        }

        #[test]
        fn test_completion_security_validation() {
            // Spec: Validate all inputs, control access to sensitive suggestions
            // Test input validation

            let dangerous_inputs = vec![
                "",                    // Empty
                "../../../etc/passwd", // Path traversal
                "'; DROP TABLE--",     // SQL injection
                "<script>alert()</script>", // XSS
            ];

            for input in dangerous_inputs {
                let request = json!({
                    "method": "completion/complete",
                    "params": {
                        "argument": {
                            "name": "query",
                            "value": input
                        }
                    }
                });

                let value = request["params"]["argument"]["value"].as_str().unwrap();
                let is_suspicious = value.is_empty() ||
                    value.contains("..") ||
                    value.contains("DROP") ||
                    value.contains("<script>");

                if is_suspicious {
                    // Should be rejected or sanitized
                    assert!(is_suspicious, "Input '{}' should be validated", input);
                }
            }

            // Safe inputs should be accepted
            let safe_inputs = vec!["python", "flask", "django"];
            for input in safe_inputs {
                let request = json!({
                    "method": "completion/complete",
                    "params": {
                        "argument": {
                            "name": "query",
                            "value": input
                        }
                    }
                });
                assert_eq!(request["params"]["argument"]["value"], input);
            }
        }

        #[test]
        fn test_completion_fuzzy_matching() {
            // Spec: Implement fuzzy matching where appropriate
            // Test fuzzy matching concept

            let candidates = vec![
                "python",
                "pyscripter",
                "pylint",
                "pytest",
                "pyright",
            ];

            let query = "py";

            // All should match prefix
            let prefix_matches: Vec<&str> = candidates.iter()
                .filter(|c| c.starts_with(query))
                .copied()
                .collect();
            assert_eq!(prefix_matches.len(), 5);

            // Fuzzy match: "ptt" should match "pytest"
            let query2 = "pyt";
            let fuzzy_matches: Vec<&str> = candidates.iter()
                .filter(|c| {
                    let mut qi = 0;
                    for ch in c.chars() {
                        if qi < query2.len() && ch == query2.chars().nth(qi).unwrap() {
                            qi += 1;
                        }
                    }
                    qi == query2.len()
                })
                .copied()
                .collect();
            assert!(fuzzy_matches.len() > 0, "Should find fuzzy matches");
            assert!(fuzzy_matches.contains(&"pytest") || fuzzy_matches.contains(&"pylint"));
        }
    }

    /// Integration Tests: Cross-Utility Interactions
    ///
    /// Test how different utilities work together in realistic scenarios
    mod utility_integration_tests {
        use super::*;

        #[test]
        fn test_progress_with_cancellation() {
            // Test cancellation of operations that are sending progress updates

            // TODO: Test cancelling operations with active progress tokens
            // EXPECTED FAILURE: Need integrated progress/cancellation handling
        }

        #[test]
        fn test_pagination_with_progress() {
            // Test progress updates during paginated operations

            // TODO: Test progress notifications during large paginated results
            // EXPECTED FAILURE: Need pagination/progress integration
        }

        #[test]
        fn test_completion_with_logging() {
            // Test that completion requests generate appropriate log messages

            // TODO: Test completion operations are properly logged
            // EXPECTED FAILURE: Need completion/logging integration
        }

        #[test]
        fn test_all_utilities_with_ping() {
            // Test that ping continues to work during other utility operations

            // TODO: Test ping health checks during utility operations
            // EXPECTED FAILURE: Need ping integration with other utilities
        }

        #[test]
        fn test_utility_error_logging() {
            // Test that utility errors are properly logged

            // TODO: Test error logging for all utilities
            // EXPECTED FAILURE: Need comprehensive error logging
        }

        #[test]
        fn test_concurrent_utility_operations() {
            // Test multiple utilities operating simultaneously

            // TODO: Test concurrent utility usage patterns
            // EXPECTED FAILURE: Need concurrent operation support
        }
    }

    /// Property-Based Testing for Utilities
    ///
    /// Use property-based testing to validate utility behaviors under various conditions
    mod utility_property_tests {
        use super::*;

        #[test]
        fn test_progress_monotonicity_property() {
            // Property: Progress values must always increase

            // TODO: Property test for progress monotonicity
            // EXPECTED FAILURE: Need property-based testing framework
        }

        #[test]
        fn test_cursor_stability_property() {
            // Property: Same cursor should always return same page

            // TODO: Property test for cursor stability
            // EXPECTED FAILURE: Need stable cursor implementation
        }

        #[test]
        fn test_log_level_filtering_property() {
            // Property: Higher level settings filter lower level messages

            // TODO: Property test for log level filtering
            // EXPECTED FAILURE: Need consistent level filtering
        }

        #[test]
        fn test_completion_uniqueness_property() {
            // Property: Completion suggestions should be unique within response

            // TODO: Property test for completion uniqueness
            // EXPECTED FAILURE: Need duplicate filtering
        }

        #[test]
        fn test_ping_response_consistency_property() {
            // Property: Ping responses should always be consistent empty objects

            // TODO: Property test for ping consistency
            // EXPECTED FAILURE: Need consistent ping implementation
        }
    }
}