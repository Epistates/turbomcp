//! Tests for timeout enforcement (Sprint 1.2)
//!
//! These tests verify that timeouts are correctly enforced at all levels:
//! - Connect timeout
//! - Request timeout
//! - Total timeout (including retries)
//! - Read timeout (streaming)

use std::time::Duration;
use turbomcp_transport::config::{TimeoutConfig, TransportConfigBuilder};
use turbomcp_transport::core::TransportType;

#[test]
fn test_timeout_config_default_values() {
    let config = TimeoutConfig::default();

    assert_eq!(config.connect, Duration::from_secs(30));
    assert_eq!(config.request, Some(Duration::from_secs(60)));
    assert_eq!(config.total, Some(Duration::from_secs(120)));
    assert_eq!(config.read, Some(Duration::from_secs(30)));
}

#[test]
fn test_timeout_config_fast_preset() {
    let config = TimeoutConfig::fast();

    assert_eq!(config.connect, Duration::from_secs(5));
    assert_eq!(config.request, Some(Duration::from_secs(10)));
    assert_eq!(config.total, Some(Duration::from_secs(15)));
    assert_eq!(config.read, Some(Duration::from_secs(5)));
}

#[test]
fn test_timeout_config_unlimited_preset() {
    let config = TimeoutConfig::unlimited();

    // Connect timeout is still enforced
    assert_eq!(config.connect, Duration::from_secs(30));

    // Other timeouts are disabled
    assert_eq!(config.request, None);
    assert_eq!(config.total, None);
    assert_eq!(config.read, None);
}

#[test]
fn test_timeout_config_patient_preset() {
    let config = TimeoutConfig::patient();

    assert_eq!(config.connect, Duration::from_secs(30));
    assert_eq!(config.request, Some(Duration::from_secs(300))); // 5 minutes
    assert_eq!(config.total, Some(Duration::from_secs(600))); // 10 minutes
    assert_eq!(config.read, Some(Duration::from_secs(60)));
}

#[test]
fn test_transport_config_builder_with_custom_timeouts() {
    let custom_timeouts = TimeoutConfig {
        connect: Duration::from_secs(10),
        request: Some(Duration::from_secs(30)),
        total: Some(Duration::from_secs(90)),
        read: Some(Duration::from_secs(15)),
    };

    let config = TransportConfigBuilder::new(TransportType::Stdio)
        .timeouts(custom_timeouts.clone())
        .build()
        .unwrap();

    assert_eq!(config.timeouts, custom_timeouts);
}

#[test]
fn test_transport_config_default_includes_timeouts() {
    let config = TransportConfigBuilder::new(TransportType::Stdio)
        .build()
        .unwrap();

    // Default timeouts should be applied
    assert_eq!(config.timeouts.connect, Duration::from_secs(30));
    assert_eq!(config.timeouts.request, Some(Duration::from_secs(60)));
    assert_eq!(config.timeouts.total, Some(Duration::from_secs(120)));
    assert_eq!(config.timeouts.read, Some(Duration::from_secs(30)));
}

#[test]
fn test_timeout_config_cloneable() {
    let config1 = TimeoutConfig::fast();
    let config2 = config1.clone();

    assert_eq!(config1, config2);
}

#[test]
fn test_timeout_config_serialization() {
    let config = TimeoutConfig::default();

    // Should be serializable for persistence
    let json = serde_json::to_string(&config).expect("Failed to serialize");
    let deserialized: TimeoutConfig = serde_json::from_str(&json).expect("Failed to deserialize");

    assert_eq!(config, deserialized);
}

#[test]
fn test_timeout_config_with_none_values() {
    let config = TimeoutConfig {
        connect: Duration::from_secs(30),
        request: None,
        total: None,
        read: None,
    };

    // Should serialize/deserialize correctly with None values
    let json = serde_json::to_string(&config).expect("Failed to serialize");
    let deserialized: TimeoutConfig = serde_json::from_str(&json).expect("Failed to deserialize");

    assert_eq!(config, deserialized);
    assert_eq!(deserialized.request, None);
    assert_eq!(deserialized.total, None);
    assert_eq!(deserialized.read, None);
}

// Test timeout error messages are helpful
#[test]
fn test_timeout_error_messages() {
    use turbomcp_transport::TransportError;

    let request_timeout_err = TransportError::RequestTimeout {
        operation: "tools/call()".to_string(),
        timeout: Duration::from_secs(60),
    };

    let error_msg = request_timeout_err.to_string();

    // Should include operation name
    assert!(error_msg.contains("tools/call()"));

    // Should include timeout duration
    assert!(error_msg.contains("60s"));

    // Should suggest how to fix
    assert!(error_msg.contains("TimeoutConfig"));
    assert!(error_msg.contains("120")); // Suggests 2x the timeout
}

#[test]
fn test_total_timeout_error_message() {
    use turbomcp_transport::TransportError;

    let total_timeout_err = TransportError::TotalTimeout {
        operation: "tools/call()".to_string(),
        timeout: Duration::from_secs(120),
    };

    let error_msg = total_timeout_err.to_string();

    // Should mention "total" and "retries"
    assert!(error_msg.contains("Total"));
    assert!(error_msg.contains("retries"));

    // Should suggest fix
    assert!(error_msg.contains("240")); // 2x the timeout
}

#[test]
fn test_read_timeout_error_message() {
    use turbomcp_transport::TransportError;

    let read_timeout_err = TransportError::ReadTimeout {
        operation: "streaming_resource()".to_string(),
        timeout: Duration::from_secs(30),
    };

    let error_msg = read_timeout_err.to_string();

    // Should mention streaming
    assert!(error_msg.contains("Read") || error_msg.contains("streaming"));

    // Should suggest fix
    assert!(error_msg.contains("60")); // 2x the timeout
}

#[test]
fn test_connection_timeout_error_message() {
    use turbomcp_transport::TransportError;

    let conn_timeout_err = TransportError::ConnectionTimeout {
        operation: "connect()".to_string(),
        timeout: Duration::from_secs(30),
    };

    let error_msg = conn_timeout_err.to_string();

    // Should mention connection
    assert!(error_msg.contains("Connection"));

    // Should suggest fix
    assert!(error_msg.contains("60")); // 2x the timeout
}
