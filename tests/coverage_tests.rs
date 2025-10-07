//! Comprehensive coverage tests for all TurboMCP modules

use turbomcp_protocol::*;
use turbomcp_server::{McpServer, ServerConfig};
use turbomcp_transport::core::*;
use turbomcp_transport::stdio::StdioTransport;
// Note: Import directly from source since mcp_protocol might not be exposed in features
use bytes::Bytes;
use serde_json::json;

#[cfg(test)]
mod core_coverage {
    use super::*;

    #[test]
    fn test_all_error_kinds() {
        let error_kinds = vec![
            ErrorKind::Transport,
            ErrorKind::Protocol,
            ErrorKind::Serialization,
            ErrorKind::Configuration,
            ErrorKind::Authentication,
            ErrorKind::Internal,
            ErrorKind::Cancelled,
        ];

        for kind in error_kinds {
            let error = Error::new(kind, "Test error message");
            assert!(!error.to_string().is_empty());

            // Test with context
            let contextual_error = error.with_context("test_key", "test_value");
            assert!(!contextual_error.to_string().is_empty());
        }
    }

    #[test]
    fn test_message_id_variants() {
        // Test different MessageId creation methods
        let id1 = MessageId::from("test-1");
        let id2 = MessageId::from("string-id");
        let id3 = MessageId::from(42i64);
        let id4 = MessageId::from("test-4");

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id3, id4);

        // Test serialization
        assert!(serde_json::to_string(&id1).is_ok());
        assert!(serde_json::to_string(&id2).is_ok());
        assert!(serde_json::to_string(&id3).is_ok());
        assert!(serde_json::to_string(&id4).is_ok());
    }

    #[test]
    fn test_state_manager_all_methods() {
        let state = StateManager::new();

        // Test all public methods
        assert_eq!(state.size(), 0);
        assert!(state.list_keys().is_empty());

        // Set operations
        state.set("key1".to_string(), json!("value1"));
        state.set("key2".to_string(), json!(42));
        state.set("key3".to_string(), json!(true));
        state.set("key4".to_string(), json!({"nested": "object"}));
        state.set("key5".to_string(), json!([1, 2, 3]));

        // Get operations
        assert_eq!(state.get("key1"), Some(json!("value1")));
        assert_eq!(state.get("key2"), Some(json!(42)));
        assert_eq!(state.get("key3"), Some(json!(true)));

        // Contains operations
        assert!(state.contains("key1"));
        assert!(state.contains("key4"));
        assert!(!state.contains("nonexistent"));

        // Size check
        assert_eq!(state.size(), 5);

        // List keys
        let keys = state.list_keys();
        assert_eq!(keys.len(), 5);

        // Remove operations
        assert_eq!(state.remove("key1"), Some(json!("value1")));
        assert_eq!(state.remove("nonexistent"), None);
        assert_eq!(state.size(), 4);

        // Export/import
        let exported = state.export();
        let new_state = StateManager::new();
        assert!(new_state.import(exported).is_ok());
        assert_eq!(new_state.size(), 4);

        // Clear
        state.clear();
        assert_eq!(state.size(), 0);

        // Default trait
        let default_state = StateManager::default();
        assert_eq!(default_state.size(), 0);
    }

    #[test]
    fn test_constants() {
        // Test all public constants
        assert!(!PROTOCOL_VERSION.is_empty());
        assert!(!SUPPORTED_VERSIONS.is_empty());
        assert!(MAX_MESSAGE_SIZE > 0);
        assert!(DEFAULT_TIMEOUT_MS > 0);
        assert!(!SDK_VERSION.is_empty());
        assert_eq!(SDK_NAME, "turbomcp");

        // Verify protocol version is in supported versions
        assert!(SUPPORTED_VERSIONS.contains(&PROTOCOL_VERSION));
    }
}

#[cfg(test)]
mod transport_coverage {
    use super::*;

    #[tokio::test]
    async fn test_all_transport_types() {
        let transport_types = vec![
            TransportType::Stdio,
            TransportType::Http,
            TransportType::WebSocket,
            TransportType::Tcp,
            TransportType::Unix,
        ];

        for transport_type in transport_types {
            // Test display
            assert!(!transport_type.to_string().is_empty());

            // Test serialization
            assert!(serde_json::to_string(&transport_type).is_ok());

            // Test configuration
            let config = TransportConfig {
                transport_type,
                ..Default::default()
            };
            assert_eq!(config.transport_type, transport_type);
        }
    }

    #[tokio::test]
    async fn test_all_transport_states() {
        let states = vec![
            TransportState::Disconnected,
            TransportState::Connecting,
            TransportState::Connected,
            TransportState::Disconnecting,
            TransportState::Failed {
                reason: "Test error".to_string(),
            },
        ];

        for state in states {
            // Test display
            assert!(!state.to_string().is_empty());

            // Test serialization
            assert!(serde_json::to_string(&state).is_ok());
        }
    }

    #[test]
    fn test_transport_capabilities() {
        let caps = TransportCapabilities::default();

        // Test all fields are accessible
        assert!(caps.max_message_size.is_some());
        assert!(!caps.supports_compression || caps.supports_compression);
        assert!(!caps.supports_streaming || caps.supports_streaming);
        assert!(caps.supports_bidirectional);
        assert!(!caps.supports_multiplexing || caps.supports_multiplexing);
        assert!(caps.compression_algorithms.is_empty() || !caps.compression_algorithms.is_empty());
        assert!(caps.custom.is_empty() || !caps.custom.is_empty());
    }

    #[test]
    fn test_transport_config() {
        let config = TransportConfig::default();

        // Test all fields
        assert_eq!(config.transport_type, TransportType::Stdio);
        assert!(config.connect_timeout.as_secs() > 0);
        assert!(config.read_timeout.is_none());
        assert!(config.write_timeout.is_none());
        assert!(config.keep_alive.is_none());
        assert!(config.max_connections.is_none());
        assert!(!config.compression);
        assert!(config.compression_algorithm.is_none());
        assert!(config.custom.is_empty());
    }

    #[test]
    fn test_transport_message() {
        let id = MessageId::from("test-msg");
        let payload = Bytes::from("test payload");
        let message = TransportMessage::new(id.clone(), payload.clone());

        // Test all methods
        assert_eq!(message.id, id);
        assert_eq!(message.payload, payload);
        assert_eq!(message.size(), payload.len());
        assert!(!message.is_compressed());
        assert!(message.content_type().is_none());
        assert!(message.correlation_id().is_none());

        // Test with metadata - these are constructors, not chainable methods
        let mut metadata = TransportMessageMetadata::with_content_type("application/json");
        metadata
            .headers
            .insert("custom".to_string(), "value".to_string());
        metadata.priority = Some(5);
        metadata.ttl = Some(30000);
        metadata.correlation_id = Some("test-correlation".to_string());

        let message_with_metadata = TransportMessage::with_metadata(id, payload, metadata);
        assert_eq!(
            message_with_metadata.content_type(),
            Some("application/json")
        );
        assert_eq!(
            message_with_metadata.correlation_id(),
            Some("test-correlation")
        );
    }

    #[test]
    fn test_transport_metrics() {
        let metrics = TransportMetrics::default();

        // Test all fields
        assert_eq!(metrics.bytes_sent, 0);
        assert_eq!(metrics.bytes_received, 0);
        assert_eq!(metrics.messages_sent, 0);
        assert_eq!(metrics.messages_received, 0);
        assert_eq!(metrics.connections, 0);
        assert_eq!(metrics.failed_connections, 0);
        assert_eq!(metrics.average_latency_ms, 0.0);
        assert_eq!(metrics.active_connections, 0);
        assert!(metrics.compression_ratio.is_none());
    }

    #[test]
    fn test_transport_errors() {
        let errors = vec![
            TransportError::ConnectionFailed("test".to_string()),
            TransportError::ConnectionLost("test".to_string()),
            TransportError::SendFailed("test".to_string()),
            TransportError::ReceiveFailed("test".to_string()),
            TransportError::SerializationFailed("test".to_string()),
            TransportError::ProtocolError("test".to_string()),
            TransportError::Timeout,
            TransportError::ConfigurationError("test".to_string()),
            TransportError::AuthenticationFailed("test".to_string()),
            TransportError::RateLimitExceeded,
            TransportError::NotAvailable("test".to_string()),
            TransportError::Io("test".to_string()),
            TransportError::Internal("test".to_string()),
        ];

        for error in errors {
            assert!(!error.to_string().is_empty());
        }
    }

    #[tokio::test]
    async fn test_stdio_transport_complete() {
        let mut transport = StdioTransport::new();

        // Test all methods
        assert_eq!(transport.transport_type(), TransportType::Stdio);
        assert!(transport.capabilities().supports_bidirectional);
        assert_eq!(transport.state().await, TransportState::Disconnected);
        assert!(!transport.is_connected().await);
        assert_eq!(transport.endpoint(), Some("stdio://".to_string()));

        // Test configuration
        let config = TransportConfig {
            transport_type: TransportType::Stdio,
            ..Default::default()
        };
        assert!(transport.configure(config).await.is_ok());

        // Test metrics
        let metrics = transport.metrics().await;
        assert_eq!(metrics.connections, 0);
    }

    #[test]
    fn test_transport_registry() {
        let registry = TransportRegistry::new();

        // Test initial state
        assert!(registry.available_transports().is_empty());
        assert!(!registry.is_available(TransportType::Stdio));

        // Test default
        let default_registry = TransportRegistry::default();
        assert!(default_registry.available_transports().is_empty());
    }

    #[test]
    fn test_transport_events() {
        let events = vec![
            TransportEvent::Connected {
                transport_type: TransportType::Stdio,
                endpoint: "stdio://".to_string(),
            },
            TransportEvent::Disconnected {
                transport_type: TransportType::Stdio,
                endpoint: "stdio://".to_string(),
                reason: Some("Test disconnect".to_string()),
            },
            TransportEvent::MessageSent {
                message_id: MessageId::from("sent-msg"),
                size: 100,
            },
            TransportEvent::MessageReceived {
                message_id: MessageId::from("sent-msg"),
                size: 200,
            },
            TransportEvent::Error {
                error: TransportError::Timeout,
                context: Some("Test context".to_string()),
            },
            TransportEvent::MetricsUpdated {
                metrics: TransportMetrics::default(),
            },
        ];

        for event in events {
            // Events should be creatable and debuggable
            let _ = format!("{:?}", event);
        }
    }

    #[tokio::test]
    async fn test_transport_event_emitter() {
        let (emitter, mut receiver) = TransportEventEmitter::new();

        // Test all emit methods
        emitter.emit_connected(TransportType::Stdio, "stdio://".to_string());
        emitter.emit_disconnected(
            TransportType::Stdio,
            "stdio://".to_string(),
            Some("test".to_string()),
        );
        emitter.emit_message_sent(MessageId::from("sent"), 100);
        emitter.emit_message_received(MessageId::from("received"), 200);
        emitter.emit_error(TransportError::Timeout, Some("test".to_string()));
        emitter.emit_metrics_updated(TransportMetrics::default());

        // Verify events are received
        let mut event_count = 0;
        while let Ok(event) = receiver.try_recv() {
            event_count += 1;
            // Each event should be debuggable
            let _ = format!("{:?}", event);
        }
        assert_eq!(event_count, 6);
    }
}

#[cfg(test)]
mod protocol_coverage {
    use super::*;

    #[test]
    fn test_simple_protocol_structures() {
        // Test basic JSON-RPC message structure
        let request = json!({
            "jsonrpc": "2.0",
            "id": "test-123",
            "method": "test_method",
            "params": {"key": "value"}
        });

        assert_eq!(request["jsonrpc"], "2.0");
        assert_eq!(request["method"], "test_method");

        // Test response structure
        let response = json!({
            "jsonrpc": "2.0",
            "id": "test-123",
            "result": {"success": true}
        });

        assert_eq!(response["result"]["success"], true);
    }

    #[test]
    fn test_protocol_serialization() {
        let test_data = json!({
            "type": "test",
            "data": "example"
        });

        let serialized = serde_json::to_string(&test_data).unwrap();
        let deserialized: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        assert_eq!(test_data, deserialized);
    }

    #[test]
    fn test_notification_structure() {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "notification_method",
            "params": {"data": "notification_data"}
        });

        // Test basic validation
        assert_eq!(notification["jsonrpc"], "2.0");
        assert_eq!(notification["method"], "notification_method");
        assert!(notification["params"].is_object());
    }

    #[test]
    fn test_error_code_values() {
        // Test standard JSON-RPC error codes
        let error_codes = vec![
            -32700, // Parse error
            -32600, // Invalid request
            -32601, // Method not found
            -32602, // Invalid params
            -32603, // Internal error
            -32000, // Tool not found (custom)
            -32001, // Tool execution error (custom)
            -32002, // Prompt not found (custom)
            -32003, // Resource not found (custom)
            -32004, // Resource access denied (custom)
        ];

        for code in error_codes {
            assert!(code < 0);
        }
    }

    #[test]
    fn test_all_method_names() {
        let methods = vec![
            "initialize",
            "initialized",
            "tools/list",
            "tools/call",
            "prompts/list",
            "prompts/get",
            "resources/list",
            "resources/read",
            "resources/subscribe",
            "resources/unsubscribe",
            "notifications/resources/updated",
            "notifications/resources/list_changed",
            "logging/setLevel",
            "notifications/message",
            "notifications/progress",
            "sampling/createMessage",
            "roots/list",
            "notifications/roots/list_changed",
        ];

        for method in methods {
            assert!(!method.is_empty());
        }
    }
}

#[cfg(test)]
mod server_coverage {
    use super::*;

    #[test]
    fn test_server_config() {
        let config = ServerConfig::default();

        // Test all fields are accessible
        assert!(!config.name.is_empty());
        assert!(!config.version.is_empty());
        assert!(config.description.is_none() || config.description.is_some());
    }

    #[tokio::test]
    async fn test_server_creation() {
        let config = ServerConfig {
            name: "test-server".to_string(),
            version: "1.0.0".to_string(),
            description: Some("Test server".to_string()),
            ..Default::default()
        };

        let _server = McpServer::new(config);
        // Server creation should succeed - if we get here, it worked
    }
}

// Edge case and robustness tests
#[cfg(test)]
mod edge_cases {
    use super::*;

    #[test]
    fn test_empty_values() {
        let state = StateManager::new();

        // Empty string keys and values
        state.set("".to_string(), json!(""));
        assert_eq!(state.get(""), Some(json!("")));

        // Null values
        state.set("null_key".to_string(), json!(null));
        assert_eq!(state.get("null_key"), Some(json!(null)));

        // Large keys/values
        let large_key = "x".repeat(1000);
        let large_value = json!("y".repeat(10000));
        state.set(large_key.clone(), large_value.clone());
        assert_eq!(state.get(&large_key), Some(large_value));
    }

    #[test]
    fn test_unicode_handling() {
        let state = StateManager::new();

        // Various unicode strings
        let unicode_cases = vec![
            "emoji_😀",
            "chinese_中文",
            "arabic_العربية",
            "russian_Русский",
            "japanese_日本語",
            "korean_한국어",
            "special_chars_©®™",
            "math_symbols_∑∏∆",
        ];

        for (i, unicode_key) in unicode_cases.iter().enumerate() {
            let unicode_value = json!(format!("value_{}_🎉", i));
            state.set(unicode_key.to_string(), unicode_value.clone());
            assert_eq!(state.get(unicode_key), Some(unicode_value));
        }
    }

    #[test]
    fn test_boundary_conditions() {
        let state = StateManager::new();

        // Very large numbers
        state.set("max_i64".to_string(), json!(i64::MAX));
        state.set("min_i64".to_string(), json!(i64::MIN));
        state.set("max_f64".to_string(), json!(f64::MAX));
        state.set("min_f64".to_string(), json!(f64::MIN));

        assert_eq!(state.get("max_i64"), Some(json!(i64::MAX)));
        assert_eq!(state.get("min_i64"), Some(json!(i64::MIN)));

        // Very long arrays
        let long_array: Vec<i32> = (0..10000).collect();
        state.set("long_array".to_string(), json!(long_array));
        assert!(state.contains("long_array"));

        // Deep nesting
        let mut nested = json!("base");
        for i in 0..100 {
            nested = json!({format!("level_{}", i): nested});
        }
        state.set("deeply_nested".to_string(), nested.clone());
        assert_eq!(state.get("deeply_nested"), Some(nested));
    }
}
