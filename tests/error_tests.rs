//! Tests for error handling

use turbomcp_protocol::{Error, Result};

#[test]
fn test_error_variants() {
    use turbomcp_protocol::ErrorKind;

    // Test Transport error
    let transport_err = Error::new(ErrorKind::Transport, "Connection failed");
    assert!(transport_err.to_string().contains("Connection failed"));

    // Test Protocol error
    let protocol_err = Error::new(ErrorKind::Protocol, "Invalid message format");
    assert!(protocol_err.to_string().contains("Invalid message format"));

    // Test Serialization error
    let serialization_err = Error::new(ErrorKind::Serialization, "JSON parse error");
    assert!(serialization_err.to_string().contains("JSON parse error"));

    // Test Configuration error
    let config_err = Error::new(ErrorKind::Configuration, "Invalid config");
    assert!(config_err.to_string().contains("Invalid config"));
}

#[test]
fn test_error_display() {
    use turbomcp_protocol::ErrorKind;

    let err = Error::new(ErrorKind::Configuration, "Invalid configuration");
    assert!(err.to_string().contains("Invalid configuration"));

    let err = Error::new(ErrorKind::Transport, "Connection failed");
    assert!(err.to_string().contains("Connection failed"));

    let err = Error::new(ErrorKind::Protocol, "Invalid message format");
    assert!(err.to_string().contains("Invalid message format"));

    let err = Error::new(ErrorKind::Serialization, "Parse error");
    assert!(err.to_string().contains("Parse error"));
}

#[test]
fn test_error_from_io() {
    use std::io;
    let io_err = io::Error::new(io::ErrorKind::NotFound, "File not found");
    let err: Error = io_err.into();

    // Check that the error contains the expected message
    assert!(err.to_string().contains("File not found"));
}

#[test]
fn test_error_from_serde() {
    let json_str = "{invalid json}";
    let parse_result: std::result::Result<serde_json::Value, _> = serde_json::from_str(json_str);

    if let Err(serde_err) = parse_result {
        let err: Error = serde_err.into();
        // Check that the error is properly converted - just verify it's not empty
        assert!(!err.to_string().is_empty());
    }
}

#[test]
fn test_result_type_alias() {
    use turbomcp_protocol::ErrorKind;

    fn test_function() -> Result<i32> {
        Ok(42)
    }

    let result = test_function();
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 42);

    fn test_error_function() -> Result<i32> {
        Err(Error::new(ErrorKind::Internal, "Test error"))
    }

    let result = test_error_function();
    assert!(result.is_err());
}

#[test]
fn test_error_context() {
    use turbomcp_protocol::ErrorKind;

    let err = Error::new(ErrorKind::Transport, "Connection timeout")
        .with_context("operation", "server_connection");

    let error_string = err.to_string();
    assert!(error_string.contains("Connection timeout"));
}
