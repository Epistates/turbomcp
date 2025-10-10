//! Functional tests for the CLI commands that actually execute the code paths
//! These tests focus on executing the actual CLI functions to achieve code coverage

use serde_json::json;
use turbomcp_cli::{Connection, TransportKind};

#[tokio::test]
async fn test_cmd_tools_list_stdio_error() {
    let conn = Connection {
        transport: Some(TransportKind::Stdio),
        command: None,
        url: "nonexistent_command".to_string(),
        auth: None,
        json: false,
    };

    // This should return an error since command execution will fail
    let result = turbomcp_cli::cmd_tools_list(conn).await;
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.contains("Failed to spawn command"));
}

#[tokio::test]
async fn test_cmd_tools_call_stdio_error() {
    let conn = Connection {
        transport: Some(TransportKind::Stdio),
        command: None,
        url: "nonexistent_command".to_string(),
        auth: None,
        json: false,
    };

    // This should return an error since command execution will fail
    let result =
        turbomcp_cli::cmd_tools_call(conn, "test_tool".to_string(), "{}".to_string()).await;
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.contains("Failed to spawn command"));
}

#[tokio::test]
async fn test_cmd_schema_export_stdio_error() {
    let conn = Connection {
        transport: Some(TransportKind::Stdio),
        command: None,
        url: "nonexistent_command".to_string(),
        auth: None,
        json: false,
    };

    // This should return an error since command execution will fail
    let result = turbomcp_cli::cmd_schema_export(conn, None).await;
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.contains("Failed to spawn command"));
}

#[tokio::test]
async fn test_http_call_tool_invalid_json() {
    let conn = Connection {
        transport: Some(TransportKind::Http),
        command: None,
        url: "http://localhost:8080/test".to_string(),
        auth: None,
        json: false,
    };

    // Test with invalid JSON arguments
    let result =
        turbomcp_cli::cmd_tools_call(conn, "test_tool".to_string(), "invalid json".to_string())
            .await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Invalid JSON arguments"));
}

#[tokio::test]
async fn test_connection_debug_format() {
    let conn = Connection {
        transport: Some(TransportKind::Http),
        command: None,
        url: "http://localhost:8080/test".to_string(),
        auth: Some("test_token".to_string()),
        json: true,
    };

    let debug_str = format!("{conn:?}");
    assert!(debug_str.contains("Connection"));
    assert!(debug_str.contains("Http"));
    assert!(debug_str.contains("http://localhost:8080/test"));
    assert!(debug_str.contains("test_token"));
    assert!(debug_str.contains("true"));
}

#[tokio::test]
async fn test_transport_kind_debug_format() {
    let stdio = TransportKind::Stdio;
    let http = TransportKind::Http;
    let ws = TransportKind::Ws;

    let stdio_str = format!("{stdio:?}");
    let http_str = format!("{http:?}");
    let ws_str = format!("{ws:?}");

    assert_eq!(stdio_str, "Stdio");
    assert_eq!(http_str, "Http");
    assert_eq!(ws_str, "Ws");
}

#[tokio::test]
async fn test_connection_clone() {
    let conn = Connection {
        transport: Some(TransportKind::Ws),
        command: None,
        url: "ws://localhost:8080/test".to_string(),
        auth: Some("token".to_string()),
        json: false,
    };

    let cloned = conn.clone();

    assert_eq!(
        format!("{:?}", conn.transport),
        format!("{:?}", cloned.transport)
    );
    assert_eq!(conn.url, cloned.url);
    assert_eq!(conn.auth, cloned.auth);
    assert_eq!(conn.json, cloned.json);
}

#[tokio::test]
async fn test_transport_kind_clone() {
    let original = TransportKind::Http;
    let cloned = original.clone();

    assert_eq!(format!("{original:?}"), format!("{:?}", cloned));
}

// Test output function with different configurations
#[test]
fn test_output_json_format() {
    let conn = Connection {
        transport: Some(TransportKind::Http),
        command: None,
        url: "test".to_string(),
        auth: None,
        json: true,
    };

    let test_value = json!({"key": "value", "number": 42});

    // This would normally print to stdout, but we can test it doesn't error
    let result = turbomcp_cli::output(&conn, &test_value);
    assert!(result.is_ok());
}

#[test]
fn test_output_non_json_format() {
    let conn = Connection {
        transport: Some(TransportKind::Http),
        command: None,
        url: "test".to_string(),
        auth: None,
        json: false,
    };

    let test_value = json!({"key": "value", "number": 42});

    // This would normally print to stdout, but we can test it doesn't error
    let result = turbomcp_cli::output(&conn, &test_value);
    assert!(result.is_ok());
}

// Test different authentication scenarios
#[tokio::test]
async fn test_connection_with_auth() {
    let conn = Connection {
        transport: Some(TransportKind::Http),
        command: None,
        url: "http://localhost:8080/test".to_string(),
        auth: Some("Bearer test_token_123".to_string()),
        json: true,
    };

    // Test that connection with auth can be created and used
    assert!(conn.auth.is_some());
    assert_eq!(conn.auth.as_ref().unwrap(), "Bearer test_token_123");
}

#[tokio::test]
async fn test_connection_without_auth() {
    let conn = Connection {
        transport: Some(TransportKind::Http),
        command: None,
        url: "http://localhost:8080/test".to_string(),
        auth: None,
        json: false,
    };

    // Test that connection without auth can be created and used
    assert!(conn.auth.is_none());
}

// Test different URL formats
#[tokio::test]
async fn test_different_url_formats() {
    let http_conn = Connection {
        transport: Some(TransportKind::Http),
        command: None,
        url: "https://api.example.com/mcp".to_string(),
        auth: None,
        json: false,
    };

    let ws_conn = Connection {
        transport: Some(TransportKind::Ws),
        command: None,
        url: "wss://api.example.com/mcp".to_string(),
        auth: None,
        json: false,
    };

    // Test that different URL formats are accepted
    assert!(http_conn.url.starts_with("https://"));
    assert!(ws_conn.url.starts_with("wss://"));
}

// Test WebSocket transport (currently mapped to HTTP)
#[tokio::test]
async fn test_websocket_transport_mapping() {
    let conn = Connection {
        transport: Some(TransportKind::Ws),
        command: None,
        url: "ws://localhost:8080/test".to_string(),
        auth: None,
        json: false,
    };

    // WebSocket commands currently delegate to HTTP implementations
    // These will fail with network error since no server is running, but we can test the code paths

    let result = turbomcp_cli::cmd_tools_list(conn.clone()).await;
    assert!(result.is_err());
    // Should fail due to network, not implementation

    let result =
        turbomcp_cli::cmd_tools_call(conn.clone(), "test".to_string(), "{}".to_string()).await;
    assert!(result.is_err());

    let result = turbomcp_cli::cmd_schema_export(conn, None).await;
    assert!(result.is_err());
}

// Test error handling for malformed JSON in arguments
#[tokio::test]
async fn test_malformed_json_arguments() {
    let conn = Connection {
        transport: Some(TransportKind::Http),
        command: None,
        url: "http://localhost:8080/test".to_string(),
        auth: None,
        json: false,
    };

    // Test various malformed JSON strings
    let malformed_jsons = vec![
        "{invalid",
        "{'key': value}",
        "{key: 'value'}",
        "[1,2,3",
        "undefined",
        "null,",
    ];

    for malformed in malformed_jsons {
        let result = turbomcp_cli::cmd_tools_call(
            conn.clone(),
            "test_tool".to_string(),
            malformed.to_string(),
        )
        .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid JSON arguments"));
    }
}

// Test valid JSON arguments (will fail on network, but JSON parsing should succeed)
#[tokio::test]
async fn test_valid_json_arguments() {
    let conn = Connection {
        transport: Some(TransportKind::Http),
        command: None,
        url: "http://localhost:8080/test".to_string(),
        auth: None,
        json: false,
    };

    let valid_jsons = vec![
        "{}",
        r#"{"key": "value"}"#,
        r#"{"number": 42, "bool": true, "null": null}"#,
        r#"{"nested": {"key": "value"}}"#,
        r#"{"array": [1, 2, 3]}"#,
    ];

    for valid_json in valid_jsons {
        let result = turbomcp_cli::cmd_tools_call(
            conn.clone(),
            "test_tool".to_string(),
            valid_json.to_string(),
        )
        .await;
        // Should fail with network error, not JSON parsing error
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(!error.contains("Invalid JSON arguments"));
    }
}
