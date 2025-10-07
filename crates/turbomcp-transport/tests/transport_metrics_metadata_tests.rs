//! Comprehensive test for TransportMetrics metadata field implementation
//!
//! This test validates the new metadata field that allows transport-specific
//! custom metrics to be stored without breaking the core metrics API.

use serde_json::json;
use std::collections::HashMap;
use turbomcp_transport::core::{AtomicMetrics, TransportMetrics};

#[test]
fn test_transport_metrics_metadata_field() {
    // Test 1: Default metrics have empty metadata
    let metrics = TransportMetrics::default();
    assert!(metrics.metadata.is_empty());

    // Test 2: Metadata can be added and retrieved
    let mut metrics = TransportMetrics::default();
    metrics
        .metadata
        .insert("session_id".to_string(), json!("test-123"));
    metrics
        .metadata
        .insert("active_correlations".to_string(), json!(42));

    assert_eq!(
        metrics.metadata.get("session_id").unwrap(),
        &json!("test-123")
    );
    assert_eq!(
        metrics.metadata.get("active_correlations").unwrap(),
        &json!(42)
    );

    // Test 3: Metadata supports all JSON types
    let mut metrics = TransportMetrics::default();
    metrics
        .metadata
        .insert("string_field".to_string(), json!("value"));
    metrics
        .metadata
        .insert("number_field".to_string(), json!(123.45));
    metrics
        .metadata
        .insert("bool_field".to_string(), json!(true));
    metrics
        .metadata
        .insert("array_field".to_string(), json!([1, 2, 3]));
    metrics
        .metadata
        .insert("object_field".to_string(), json!({"key": "value"}));
    metrics
        .metadata
        .insert("null_field".to_string(), json!(null));

    assert_eq!(metrics.metadata.len(), 6);

    // Test 4: AtomicMetrics snapshot has empty metadata
    let atomic = AtomicMetrics::new();
    let snapshot = atomic.snapshot();
    assert!(snapshot.metadata.is_empty());

    // Test 5: Serialization omits empty metadata
    let metrics = TransportMetrics::default();
    let serialized = serde_json::to_value(&metrics).unwrap();
    assert!(!serialized.as_object().unwrap().contains_key("metadata"));

    // Test 6: Serialization includes non-empty metadata
    let mut metrics = TransportMetrics::default();
    metrics
        .metadata
        .insert("custom".to_string(), json!("value"));
    let serialized = serde_json::to_value(&metrics).unwrap();
    assert!(serialized.as_object().unwrap().contains_key("metadata"));
    assert_eq!(serialized["metadata"]["custom"], json!("value"));

    // Test 7: Deserialization handles missing metadata
    let json_str = r#"{
        "bytes_sent": 100,
        "bytes_received": 200,
        "messages_sent": 10,
        "messages_received": 20,
        "connections": 1,
        "failed_connections": 0,
        "average_latency_ms": 1.5,
        "active_connections": 1,
        "compression_ratio": 2.0
    }"#;
    let metrics: TransportMetrics = serde_json::from_str(json_str).unwrap();
    assert!(metrics.metadata.is_empty());

    // Test 8: Deserialization handles present metadata
    let json_str = r#"{
        "bytes_sent": 100,
        "bytes_received": 200,
        "messages_sent": 10,
        "messages_received": 20,
        "connections": 1,
        "failed_connections": 0,
        "average_latency_ms": 1.5,
        "active_connections": 1,
        "compression_ratio": 2.0,
        "metadata": {
            "session_id": "abc123",
            "active_correlations": 5
        }
    }"#;
    let metrics: TransportMetrics = serde_json::from_str(json_str).unwrap();
    assert_eq!(metrics.metadata.len(), 2);
    assert_eq!(
        metrics.metadata.get("session_id").unwrap(),
        &json!("abc123")
    );
    assert_eq!(
        metrics.metadata.get("active_correlations").unwrap(),
        &json!(5)
    );
}

#[test]
fn test_websocket_metadata_usage() {
    // Simulate WebSocket-specific metrics
    let mut metrics = TransportMetrics {
        bytes_sent: 1024,
        bytes_received: 2048,
        messages_sent: 10,
        messages_received: 15,
        connections: 1,
        failed_connections: 0,
        average_latency_ms: 5.2,
        active_connections: 1,
        compression_ratio: Some(1.8),
        metadata: HashMap::new(),
    };

    // Add WebSocket-specific fields
    metrics
        .metadata
        .insert("active_correlations".to_string(), json!(3));
    metrics
        .metadata
        .insert("pending_elicitations".to_string(), json!(2));
    metrics
        .metadata
        .insert("session_id".to_string(), json!("ws-session-789"));
    metrics
        .metadata
        .insert("max_message_size".to_string(), json!(16384));
    metrics
        .metadata
        .insert("keep_alive_interval_secs".to_string(), json!(30));
    metrics
        .metadata
        .insert("max_frame_size".to_string(), json!(65536));

    // Verify all fields present
    assert_eq!(metrics.metadata.len(), 6);

    // Verify values are correct types
    assert!(
        metrics
            .metadata
            .get("active_correlations")
            .unwrap()
            .is_number()
    );
    assert!(metrics.metadata.get("session_id").unwrap().is_string());

    // Verify serialization includes all fields
    let serialized = serde_json::to_value(&metrics).unwrap();
    let metadata = &serialized["metadata"];
    assert_eq!(metadata["active_correlations"], json!(3));
    assert_eq!(metadata["session_id"], json!("ws-session-789"));
}

#[test]
fn test_metadata_backward_compatibility() {
    // Old code that doesn't set metadata should still work
    let metrics = TransportMetrics {
        bytes_sent: 1000,
        bytes_received: 2000,
        messages_sent: 5,
        messages_received: 10,
        connections: 2,
        failed_connections: 1,
        average_latency_ms: 3.5,
        active_connections: 1,
        compression_ratio: None,
        metadata: HashMap::new(), // Empty metadata
    };

    // Should serialize without metadata field
    let serialized = serde_json::to_value(&metrics).unwrap();
    assert!(!serialized.as_object().unwrap().contains_key("metadata"));

    // Should deserialize from old format
    let old_format = r#"{
        "bytes_sent": 1000,
        "bytes_received": 2000,
        "messages_sent": 5,
        "messages_received": 10,
        "connections": 2,
        "failed_connections": 1,
        "average_latency_ms": 3.5,
        "active_connections": 1
    }"#;
    let deserialized: TransportMetrics = serde_json::from_str(old_format).unwrap();
    assert_eq!(deserialized.bytes_sent, 1000);
    assert!(deserialized.metadata.is_empty());
}

#[test]
fn test_metadata_clone_and_debug() {
    let mut metrics = TransportMetrics::default();
    metrics.metadata.insert("test".to_string(), json!("value"));

    // Test Clone
    let cloned = metrics.clone();
    assert_eq!(cloned.metadata.get("test").unwrap(), &json!("value"));

    // Test Debug
    let debug_str = format!("{:?}", metrics);
    assert!(debug_str.contains("metadata"));
}
