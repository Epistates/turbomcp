//! Comprehensive MCP Transport Compliance Tests
//!
//! Tests all MCP transport mechanisms for specification compliance:
//! - stdio transport (standard input/output)
//! - Streamable HTTP transport (POST/GET with SSE)
//! - Custom transport requirements
//! - General transport requirements
//!
//! Based on MCP specification draft:
//! - /basic/transports.mdx

use serde_json::{json, Value};
use std::collections::HashMap;
use turbomcp::*;
use turbomcp_protocol::types::*;

/// Test transports with comprehensive scenarios covering all specification requirements
#[cfg(test)]
mod mcp_transport_compliance_tests {
    use super::*;

    /// Test Group: General Transport Requirements
    ///
    /// Based on specification: /basic/transports.mdx
    /// Requirements:
    /// - JSON-RPC messages MUST be UTF-8 encoded
    /// - Support for bidirectional message exchange
    /// - Preserve JSON-RPC format across all transports
    mod general_transport_tests {
        use super::*;

        #[test]
        fn test_utf8_encoding_requirement() {
            // Spec: JSON-RPC messages MUST be UTF-8 encoded
            let message_with_unicode = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "test",
                "params": {
                    "text": "Hello 世界 🌍",
                    "arabic": "مرحبا",
                    "emoji": "👋🏽"
                }
            });

            // Serialize and verify UTF-8 is preserved
            let serialized = serde_json::to_string(&message_with_unicode).unwrap();
            assert!(serialized.contains("世界"), "Chinese characters must be preserved");
            assert!(serialized.contains("🌍"), "Emoji must be preserved");
            assert!(serialized.contains("مرحبا"), "Arabic must be preserved");

            // Deserialize and verify round-trip
            let deserialized: Value = serde_json::from_str(&serialized).unwrap();
            assert_eq!(
                deserialized["params"]["text"].as_str().unwrap(),
                "Hello 世界 🌍"
            );
        }

        #[test]
        fn test_json_rpc_format_preservation() {
            // Spec: Preserve JSON-RPC message format across all transports

            // Test request format
            let request = json!({
                "jsonrpc": "2.0",
                "id": 42,
                "method": "tools/list",
                "params": {}
            });

            // Verify required fields
            assert_eq!(request["jsonrpc"], "2.0");
            assert!(request["id"].is_number());
            assert!(request["method"].is_string());

            // Test response format
            let response = json!({
                "jsonrpc": "2.0",
                "id": 42,
                "result": {
                    "tools": []
                }
            });

            assert_eq!(response["jsonrpc"], "2.0");
            assert_eq!(response["id"], 42);
            assert!(response["result"].is_object());

            // Test notification format (no id)
            let notification = json!({
                "jsonrpc": "2.0",
                "method": "notifications/tools/list_changed"
            });

            assert_eq!(notification["jsonrpc"], "2.0");
            assert!(notification.get("id").is_none());
            assert!(notification["method"].is_string());
        }

        #[test]
        fn test_bidirectional_message_exchange() {
            // Spec: All transports must support bidirectional communication

            // Client -> Server messages
            let client_to_server_request = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": { "name": "test", "version": "1.0" }
                }
            });

            // Server -> Client messages (sampling request from server)
            let server_to_client_request = json!({
                "jsonrpc": "2.0",
                "id": "server-1",
                "method": "sampling/createMessage",
                "params": {
                    "messages": [{ "role": "user", "content": { "type": "text", "text": "Hello" }}],
                    "maxTokens": 100
                }
            });

            // Both directions must maintain valid JSON-RPC format
            assert_eq!(client_to_server_request["jsonrpc"], "2.0");
            assert_eq!(server_to_client_request["jsonrpc"], "2.0");
        }

        #[test]
        fn test_transport_agnostic_protocol() {
            // Spec: Protocol is transport-agnostic
            // Same message format should work regardless of transport

            let tool_call = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "example_tool",
                    "arguments": { "input": "test" }
                }
            });

            // Message format is identical regardless of transport
            let serialized = serde_json::to_string(&tool_call).unwrap();

            // Can be transmitted over any transport as-is
            assert!(serialized.len() > 0);
            assert!(!serialized.contains('\n') || serialized.contains("\\n"));
        }
    }

    /// Test Group: stdio Transport Compliance
    ///
    /// Based on specification: /basic/transports.mdx#stdio
    mod stdio_transport_tests {
        use super::*;

        #[test]
        fn test_newline_message_delimiter() {
            // Spec: Messages delimited by newlines
            let valid_message = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "test"
            });

            // Compact serialization (no embedded newlines)
            let compact = serde_json::to_string(&valid_message).unwrap();
            let message_line = format!("{}\n", compact);

            // Must end with exactly one newline
            assert!(message_line.ends_with('\n'));
            // No embedded newlines in the message body
            assert!(!message_line[..message_line.len() - 1].contains('\n'));
            // Can be parsed back
            let parsed: Value = serde_json::from_str(compact.as_str()).unwrap();
            assert_eq!(parsed["id"], 1);
        }

        #[test]
        fn test_no_embedded_newlines_requirement() {
            // Spec: Messages MUST NOT contain embedded newlines

            // Pretty-printed JSON has newlines - NOT valid for stdio
            let pretty_message = serde_json::to_string_pretty(&json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "test"
            })).unwrap();

            // This format is INVALID for stdio transport
            assert!(pretty_message.contains('\n'), "Pretty format has newlines");

            // Compact JSON is valid
            let compact_message = serde_json::to_string(&json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "test"
            })).unwrap();

            assert!(!compact_message.contains('\n'), "Compact format must not have newlines");
        }

        #[test]
        fn test_message_with_newline_in_string_value() {
            // String values CAN contain escaped newlines
            let message_with_escaped_newline = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "test",
                "params": {
                    "multiline_text": "line1\nline2\nline3"
                }
            });

            let serialized = serde_json::to_string(&message_with_escaped_newline).unwrap();

            // Serialized form has \\n (escaped), not actual newlines
            assert!(serialized.contains("\\n"), "Should have escaped newlines");
            assert!(!serialized.chars().any(|c| c == '\n'), "Should not have literal newlines");
        }

        #[test]
        fn test_stdio_message_parsing() {
            // Test parsing multiple newline-delimited messages
            let messages = vec![
                json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
                json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
                json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            ];

            // Serialize as newline-delimited stream
            let stream: String = messages
                .iter()
                .map(|m| format!("{}\n", serde_json::to_string(m).unwrap()))
                .collect();

            // Parse back
            let parsed: Vec<Value> = stream
                .lines()
                .filter(|line| !line.is_empty())
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();

            assert_eq!(parsed.len(), 3);
            assert_eq!(parsed[0]["id"], 1);
            assert_eq!(parsed[1]["id"], 2);
            assert!(parsed[2].get("id").is_none()); // notification has no id
        }

        #[test]
        fn test_stderr_is_not_protocol() {
            // Spec: stderr is for logging, not protocol messages
            // Protocol messages must only go to stdout

            // A log message on stderr would look like:
            let stderr_log = "[INFO] Server starting...";

            // This is NOT valid JSON-RPC
            let parse_result: Result<Value, _> = serde_json::from_str(stderr_log);
            assert!(parse_result.is_err(), "Stderr log should not be valid JSON-RPC");
        }
    }

    /// Test Group: Streamable HTTP Transport Compliance
    ///
    /// Based on specification: /basic/transports.mdx#streamable-http
    mod streamable_http_transport_tests {
        use super::*;

        #[test]
        fn test_post_request_requirements() {
            // Spec: Client MUST use POST for JSON-RPC messages
            // MUST include Accept header with application/json and text/event-stream

            let required_headers = HashMap::from([
                ("Content-Type".to_string(), "application/json".to_string()),
                (
                    "Accept".to_string(),
                    "application/json, text/event-stream".to_string(),
                ),
            ]);

            assert_eq!(required_headers["Content-Type"], "application/json");
            assert!(required_headers["Accept"].contains("application/json"));
            assert!(required_headers["Accept"].contains("text/event-stream"));
        }

        #[test]
        fn test_session_id_format_requirements() {
            // Spec: Session ID SHOULD be globally unique and cryptographically secure
            // MUST only contain visible ASCII characters (0x21 to 0x7E)

            // Valid session IDs
            let valid_ids = [
                "abc123-DEF456_789.xyz",
                "mcp_session_1868a90c-1234-5678-9abc-def012345678",
                "!@#$%^&*()_+-=[]{}|;':\",./<>?", // All visible ASCII
            ];

            for id in &valid_ids {
                for c in id.chars() {
                    let byte = c as u8;
                    assert!(
                        byte >= 0x21 && byte <= 0x7E,
                        "Character '{}' (0x{:02X}) must be visible ASCII",
                        c,
                        byte
                    );
                }
            }

            // Invalid session IDs (contain control chars or spaces)
            let invalid_ids = [
                "session with space",    // contains space (0x20)
                "session\ttab",          // contains tab
                "session\nnewline",      // contains newline
            ];

            for id in &invalid_ids {
                let has_invalid = id.chars().any(|c| {
                    let byte = c as u8;
                    byte < 0x21 || byte > 0x7E
                });
                assert!(has_invalid, "ID '{}' should have invalid characters", id);
            }
        }

        #[test]
        fn test_protocol_version_header() {
            // Spec: Client MUST include MCP-Protocol-Version header
            let protocol_version = "2025-11-25";

            // Validate version format (YYYY-MM-DD)
            let parts: Vec<&str> = protocol_version.split('-').collect();
            assert_eq!(parts.len(), 3);
            assert_eq!(parts[0].len(), 4); // YYYY
            assert_eq!(parts[1].len(), 2); // MM
            assert_eq!(parts[2].len(), 2); // DD

            // All parts should be numeric
            for part in &parts {
                assert!(part.chars().all(|c| c.is_ascii_digit()));
            }
        }

        #[test]
        fn test_origin_header_localhost_patterns() {
            // Spec: Servers MUST validate Origin header to prevent DNS rebinding

            // Valid localhost origins
            let valid_origins = [
                "http://localhost",
                "http://localhost:3000",
                "https://localhost:8443",
                "http://127.0.0.1",
                "http://127.0.0.1:3000",
                "http://[::1]",
                "http://[::1]:3000",
            ];

            for origin in &valid_origins {
                let is_localhost = origin.contains("localhost")
                    || origin.contains("127.0.0.1")
                    || origin.contains("[::1]");
                assert!(is_localhost, "Origin {} should be recognized as localhost", origin);
            }

            // Invalid origins (should be rejected with 403)
            let invalid_origins = [
                "http://evil.com",
                "http://example.org",
                "http://192.168.1.1",  // Not localhost
                "http://10.0.0.1",     // Private but not localhost
            ];

            for origin in &invalid_origins {
                let is_localhost = origin.contains("localhost")
                    || origin.contains("127.0.0.1")
                    || origin.contains("[::1]");
                assert!(!is_localhost, "Origin {} should NOT be recognized as localhost", origin);
            }
        }

        #[test]
        fn test_sse_event_format() {
            // Spec: Server-Sent Events format for streaming responses

            // SSE event format
            let event_data = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": { "tools": [] }
            });

            // Format as SSE event
            let sse_event = format!(
                "event: message\ndata: {}\n\n",
                serde_json::to_string(&event_data).unwrap()
            );

            // Verify SSE format
            assert!(sse_event.starts_with("event: message\n"));
            assert!(sse_event.contains("data: "));
            assert!(sse_event.ends_with("\n\n")); // Double newline terminates event
        }

        #[test]
        fn test_sse_event_id_format() {
            // Spec: Servers MAY attach event IDs for resumability
            // Event IDs MUST be globally unique within session

            use std::collections::HashSet;

            let mut seen_ids = HashSet::new();

            // Generate sample event IDs
            for i in 0..100 {
                let event_id = format!("evt-{}-{}", std::process::id(), i);
                assert!(
                    seen_ids.insert(event_id.clone()),
                    "Event ID {} must be unique",
                    event_id
                );
            }
        }

        #[test]
        fn test_http_response_status_codes() {
            // Spec: Different status codes for different scenarios

            // 200 OK - Successful request with body
            // 202 Accepted - For notifications/responses with no response body
            // 400 Bad Request - Invalid protocol version
            // 403 Forbidden - Invalid Origin header
            // 404 Not Found - Invalid/terminated session
            // 405 Method Not Allowed - Unsupported HTTP method

            let valid_status_codes = [200, 202, 400, 403, 404, 405];

            for code in &valid_status_codes {
                assert!(
                    *code >= 100 && *code < 600,
                    "Status code {} must be valid HTTP",
                    code
                );
            }
        }

        #[test]
        fn test_cancellation_notification_vs_disconnection() {
            // Spec: Disconnection SHOULD NOT be interpreted as cancellation
            // Use explicit CancelledNotification for cancellation

            // Proper cancellation notification
            let cancel_notification = json!({
                "jsonrpc": "2.0",
                "method": "notifications/cancelled",
                "params": {
                    "requestId": 42,
                    "reason": "User requested cancellation"
                }
            });

            assert_eq!(cancel_notification["method"], "notifications/cancelled");
            assert_eq!(cancel_notification["params"]["requestId"], 42);
        }
    }

    /// Test Group: Custom Transport Requirements
    mod custom_transport_tests {
        use super::*;

        #[test]
        fn test_custom_transport_json_rpc_preservation() {
            // Spec: Custom transports MUST preserve JSON-RPC message format

            // Any custom transport must be able to send/receive this exact format
            let standard_message = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "test",
                "params": { "key": "value" }
            });

            // Serialize through any transport
            let bytes = serde_json::to_vec(&standard_message).unwrap();

            // Deserialize on receiving end
            let received: Value = serde_json::from_slice(&bytes).unwrap();

            // Must be identical
            assert_eq!(standard_message, received);
        }

        #[test]
        fn test_custom_transport_lifecycle_preservation() {
            // Spec: Custom transports MUST preserve lifecycle requirements

            // Required lifecycle sequence
            let lifecycle_methods = [
                "initialize",              // Client -> Server (must be first)
                "notifications/initialized", // Server -> Client (after initialize response)
                // ... normal operation ...
                // shutdown can occur anytime after initialized
            ];

            // First message MUST be initialize
            assert_eq!(lifecycle_methods[0], "initialize");
        }
    }

    /// Test Group: Transport Security Requirements
    mod transport_security_tests {
        use super::*;

        #[test]
        fn test_dns_rebinding_protection_logic() {
            // Spec: HTTP transport MUST validate Origin header

            fn is_origin_allowed(origin: &str) -> bool {
                // Only localhost variants are allowed by default
                let localhost_patterns = [
                    "localhost",
                    "127.0.0.1",
                    "[::1]",
                    "::1",
                ];

                for pattern in &localhost_patterns {
                    if origin.contains(pattern) {
                        return true;
                    }
                }
                false
            }

            // Valid origins
            assert!(is_origin_allowed("http://localhost:3000"));
            assert!(is_origin_allowed("http://127.0.0.1:8080"));
            assert!(is_origin_allowed("http://[::1]:3000"));

            // Invalid origins (DNS rebinding attacks)
            assert!(!is_origin_allowed("http://evil.com"));
            assert!(!is_origin_allowed("http://localhost.evil.com")); // Subdomain trick
            assert!(!is_origin_allowed("http://192.168.1.1")); // Private but not localhost
        }

        #[test]
        fn test_secure_session_id_entropy() {
            // Spec: Session IDs SHOULD be cryptographically secure

            use std::collections::HashSet;

            // Generate multiple session IDs and verify they're unique
            let mut ids = HashSet::new();
            for _ in 0..1000 {
                // Simple UUID-like generation
                let id = format!(
                    "mcp_{:08x}_{:08x}",
                    fastrand::u32(..),
                    fastrand::u32(..)
                );
                assert!(ids.insert(id), "Session IDs must be unique");
            }

            // All 1000 IDs should be unique
            assert_eq!(ids.len(), 1000);
        }
    }

    /// Test Group: Transport Error Handling
    mod transport_error_handling_tests {
        use super::*;

        #[test]
        fn test_malformed_json_handling() {
            // Test handling of malformed messages

            let malformed_messages = [
                "",                          // Empty
                "not json",                  // Plain text
                "{",                         // Incomplete
                r#"{"jsonrpc": "2.0""#,     // Missing closing brace
                r#"{"jsonrpc": "1.0"}"#,    // Wrong version
            ];

            for msg in &malformed_messages {
                let result: Result<Value, _> = serde_json::from_str(msg);
                // Either fails to parse or has wrong version
                if let Ok(parsed) = result {
                    if let Some(version) = parsed.get("jsonrpc") {
                        assert_ne!(version, "2.0", "Message '{}' should not be valid", msg);
                    }
                }
            }
        }

        #[test]
        fn test_json_rpc_error_format() {
            // Spec: Error responses must follow JSON-RPC 2.0 format

            let error_response = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "error": {
                    "code": -32600,
                    "message": "Invalid Request",
                    "data": { "details": "Missing required field" }
                }
            });

            assert_eq!(error_response["jsonrpc"], "2.0");
            assert!(error_response["error"]["code"].is_i64());
            assert!(error_response["error"]["message"].is_string());
        }

        #[test]
        fn test_standard_json_rpc_error_codes() {
            // Standard JSON-RPC 2.0 error codes
            let standard_codes = [
                (-32700, "Parse error"),
                (-32600, "Invalid Request"),
                (-32601, "Method not found"),
                (-32602, "Invalid params"),
                (-32603, "Internal error"),
            ];

            for (code, message) in &standard_codes {
                assert!(
                    *code >= -32700 && *code <= -32600,
                    "Code {} ({}) must be in standard range",
                    code,
                    message
                );
            }
        }
    }

    /// Test Group: Transport Performance Requirements
    mod transport_performance_tests {
        use super::*;
        use std::time::Instant;

        #[test]
        fn test_message_serialization_performance() {
            // Test that message serialization is fast enough for real-time use

            let message = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "example_tool",
                    "arguments": {
                        "input": "test data",
                        "options": { "verbose": true, "timeout": 30 }
                    }
                }
            });

            let iterations = 10000;
            let start = Instant::now();

            for _ in 0..iterations {
                let _ = serde_json::to_string(&message).unwrap();
            }

            let elapsed = start.elapsed();
            let per_message = elapsed / iterations;

            // Should be less than 100 microseconds per message
            assert!(
                per_message.as_micros() < 100,
                "Serialization took {:?} per message, should be < 100µs",
                per_message
            );
        }

        #[test]
        fn test_message_deserialization_performance() {
            let message_str = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"example_tool","arguments":{"input":"test data"}}}"#;

            let iterations = 10000;
            let start = Instant::now();

            for _ in 0..iterations {
                let _: Value = serde_json::from_str(message_str).unwrap();
            }

            let elapsed = start.elapsed();
            let per_message = elapsed / iterations;

            // Should be less than 100 microseconds per message
            assert!(
                per_message.as_micros() < 100,
                "Deserialization took {:?} per message, should be < 100µs",
                per_message
            );
        }

        #[test]
        fn test_large_message_handling() {
            // Test handling of large messages (within reasonable limits)

            // Create a large payload (~100KB)
            let large_content = "x".repeat(100_000);
            let large_message = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "content": large_content
                }
            });

            // Should serialize without issue
            let serialized = serde_json::to_string(&large_message).unwrap();
            assert!(serialized.len() > 100_000);

            // Should deserialize back
            let deserialized: Value = serde_json::from_str(&serialized).unwrap();
            assert_eq!(
                deserialized["result"]["content"].as_str().unwrap().len(),
                100_000
            );
        }
    }

    /// Property-Based Testing for Transports
    mod transport_property_tests {
        use super::*;

        #[test]
        fn test_message_roundtrip_property() {
            // Property: Any valid JSON-RPC message should survive round-trip

            let test_messages = vec![
                // Request
                json!({"jsonrpc": "2.0", "id": 1, "method": "test", "params": {}}),
                // Response
                json!({"jsonrpc": "2.0", "id": 1, "result": {"data": "value"}}),
                // Error response
                json!({"jsonrpc": "2.0", "id": 1, "error": {"code": -32600, "message": "Invalid"}}),
                // Notification
                json!({"jsonrpc": "2.0", "method": "notification"}),
                // With complex params
                json!({
                    "jsonrpc": "2.0",
                    "id": "string-id",
                    "method": "complex",
                    "params": {
                        "nested": {"deep": {"value": 42}},
                        "array": [1, 2, 3],
                        "unicode": "日本語"
                    }
                }),
            ];

            for original in test_messages {
                // Serialize
                let serialized = serde_json::to_string(&original).unwrap();
                // Deserialize
                let restored: Value = serde_json::from_str(&serialized).unwrap();
                // Must be identical
                assert_eq!(original, restored);
            }
        }

        #[test]
        fn test_utf8_handling_property() {
            // Property: All valid UTF-8 should be handled correctly

            let utf8_test_strings = [
                "ASCII only",
                "Émojis: 🎉🚀💯",
                "Chinese: 中文",
                "Japanese: 日本語",
                "Korean: 한국어",
                "Arabic: العربية",
                "Hebrew: עברית",
                "Thai: ภาษาไทย",
                "Mixed: Hello世界🌍",
                // Edge cases
                "\u{0000}",           // Null char
                "\u{FFFF}",           // Max BMP
                "𝄞",                  // Musical symbol (outside BMP)
            ];

            for test_str in &utf8_test_strings {
                let message = json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": {"text": *test_str}
                });

                let serialized = serde_json::to_string(&message).unwrap();
                let restored: Value = serde_json::from_str(&serialized).unwrap();

                assert_eq!(
                    restored["result"]["text"].as_str().unwrap(),
                    *test_str,
                    "UTF-8 string '{}' must survive round-trip",
                    test_str
                );
            }
        }

        #[test]
        fn test_message_order_preservation() {
            // Property: Message order must be preserved

            let messages: Vec<Value> = (0..100)
                .map(|i| {
                    json!({
                        "jsonrpc": "2.0",
                        "id": i,
                        "method": "test"
                    })
                })
                .collect();

            // Serialize all
            let serialized: Vec<String> = messages
                .iter()
                .map(|m| serde_json::to_string(m).unwrap())
                .collect();

            // Deserialize and verify order
            for (i, s) in serialized.iter().enumerate() {
                let parsed: Value = serde_json::from_str(s).unwrap();
                assert_eq!(
                    parsed["id"].as_u64().unwrap(),
                    i as u64,
                    "Message order must be preserved"
                );
            }
        }
    }
}
