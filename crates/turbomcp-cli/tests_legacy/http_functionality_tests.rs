//! Tests for HTTP functionality and edge cases in the CLI

use clap::Parser;
use serde_json::json;
use turbomcp_cli::{Cli, Commands, Connection, TransportKind};

#[test]
fn test_cli_parsing_tools_list() {
    let args = vec![
        "turbomcp-cli",
        "tools-list",
        "--transport",
        "http",
        "--url",
        "http://localhost:8080/mcp",
        "--auth",
        "bearer_token_123",
        "--json",
    ];

    let cli = Cli::try_parse_from(args).expect("Failed to parse CLI args");

    match cli.command {
        Commands::ToolsList(conn) => {
            assert!(matches!(conn.transport, Some(TransportKind::Http)));
            assert_eq!(conn.url, "http://localhost:8080/mcp");
            assert_eq!(conn.auth.as_ref().unwrap(), "bearer_token_123");
            assert!(conn.json);
        }
        _ => panic!("Expected ToolsList command"),
    }
}

#[test]
fn test_cli_parsing_tools_call() {
    let args = vec![
        "turbomcp-cli",
        "tools-call",
        "--transport",
        "ws",
        "--url",
        "ws://localhost:8080/mcp",
        "--name",
        "add_numbers",
        "--arguments",
        r#"{"a": 5, "b": 3}"#,
    ];

    let cli = Cli::try_parse_from(args).expect("Failed to parse CLI args");

    match cli.command {
        Commands::ToolsCall {
            conn,
            name,
            arguments,
        } => {
            assert!(matches!(conn.transport, Some(TransportKind::Ws)));
            assert_eq!(conn.url, "ws://localhost:8080/mcp");
            assert!(conn.auth.is_none());
            assert!(!conn.json);
            assert_eq!(name, "add_numbers");
            assert_eq!(arguments, r#"{"a": 5, "b": 3}"#);
        }
        _ => panic!("Expected ToolsCall command"),
    }
}

#[test]
fn test_cli_parsing_schema_export() {
    let args = vec![
        "turbomcp-cli",
        "schema-export",
        "--transport",
        "stdio",
        "--json",
    ];

    let cli = Cli::try_parse_from(args).expect("Failed to parse CLI args");

    match cli.command {
        Commands::SchemaExport { conn, .. } => {
            assert!(matches!(conn.transport, Some(TransportKind::Stdio)));
            assert_eq!(conn.url, "http://localhost:8080/mcp"); // default
            assert!(conn.auth.is_none());
            assert!(conn.json);
        }
        _ => panic!("Expected SchemaExport command"),
    }
}

#[test]
fn test_cli_parsing_with_defaults() {
    let args = vec!["turbomcp-cli", "tools-list"];

    let cli = Cli::try_parse_from(args).expect("Failed to parse CLI args");

    match cli.command {
        Commands::ToolsList(conn) => {
            assert!(conn.transport.is_none()); // None means auto-detection
            assert_eq!(conn.url, "http://localhost:8080/mcp"); // default
            assert!(conn.auth.is_none());
            assert!(!conn.json);
        }
        _ => panic!("Expected ToolsList command"),
    }
}

#[test]
fn test_cli_parsing_tools_call_with_defaults() {
    let args = vec![
        "turbomcp-cli",
        "tools-call",
        "--name",
        "test_tool", // arguments will use default "{}"
    ];

    let cli = Cli::try_parse_from(args).expect("Failed to parse CLI args");

    match cli.command {
        Commands::ToolsCall {
            conn,
            name,
            arguments,
        } => {
            assert!(conn.transport.is_none()); // None means auto-detection
            assert_eq!(conn.url, "http://localhost:8080/mcp"); // default
            assert!(conn.auth.is_none());
            assert!(!conn.json);
            assert_eq!(name, "test_tool");
            assert_eq!(arguments, "{}"); // default
        }
        _ => panic!("Expected ToolsCall command"),
    }
}

// Test different transport types
#[test]
fn test_transport_kind_variants() {
    let stdio = TransportKind::Stdio;
    let http = TransportKind::Http;
    let ws = TransportKind::Ws;

    // Test that they're different
    assert_ne!(format!("{stdio:?}"), format!("{:?}", http));
    assert_ne!(format!("{http:?}"), format!("{:?}", ws));
    assert_ne!(format!("{stdio:?}"), format!("{:?}", ws));

    // Test cloning
    let stdio_clone = stdio.clone();
    let http_clone = http.clone();
    let ws_clone = ws.clone();

    assert_eq!(format!("{stdio:?}"), format!("{:?}", stdio_clone));
    assert_eq!(format!("{http:?}"), format!("{:?}", http_clone));
    assert_eq!(format!("{ws:?}"), format!("{:?}", ws_clone));
}

// Test Connection struct methods and properties
#[test]
fn test_connection_comprehensive() {
    let conn = Connection {
        transport: Some(TransportKind::Http),
        command: None,
        url: "https://api.example.com/mcp".to_string(),
        auth: Some("api_key_12345".to_string()),
        json: true,
    };

    // Test Debug formatting
    let debug_str = format!("{conn:?}");
    assert!(debug_str.contains("Connection"));
    assert!(debug_str.contains("Http"));
    assert!(debug_str.contains("https://api.example.com/mcp"));
    assert!(debug_str.contains("api_key_12345"));
    assert!(debug_str.contains("true"));

    // Test Clone
    let cloned = conn.clone();
    assert_eq!(
        format!("{:?}", conn.transport),
        format!("{:?}", cloned.transport)
    );
    assert_eq!(conn.url, cloned.url);
    assert_eq!(conn.auth, cloned.auth);
    assert_eq!(conn.json, cloned.json);
}

// Test Args derive for Connection
#[test]
fn test_connection_args_parsing() {
    use clap::Parser;

    #[derive(Parser)]
    struct TestArgs {
        #[command(flatten)]
        connection: Connection,
    }

    let args = vec![
        "test",
        "--transport",
        "http",
        "--url",
        "http://test.com",
        "--auth",
        "token123",
        "--json",
    ];

    let parsed = TestArgs::try_parse_from(args).expect("Failed to parse args");

    assert!(matches!(
        parsed.connection.transport,
        Some(TransportKind::Http)
    ));
    assert_eq!(parsed.connection.url, "http://test.com");
    assert_eq!(parsed.connection.auth.as_ref().unwrap(), "token123");
    assert!(parsed.connection.json);
}

// Test error cases for CLI parsing
#[test]
fn test_cli_parsing_invalid_transport() {
    let args = vec![
        "turbomcp-cli",
        "tools-list",
        "--transport",
        "invalid_transport",
    ];

    let result = Cli::try_parse_from(args);
    assert!(result.is_err(), "Should fail with invalid transport");

    let error = result.unwrap_err();
    let error_str = error.to_string();
    assert!(error_str.contains("invalid_transport") || error_str.contains("invalid"));
}

#[test]
fn test_cli_parsing_missing_required_args() {
    let args = vec!["turbomcp-cli"];

    let result = Cli::try_parse_from(args);
    assert!(result.is_err(), "Should fail without subcommand");

    let error = result.unwrap_err();
    let error_str = error.to_string();
    assert!(error_str.contains("required") || error_str.contains("subcommand"));
}

#[test]
fn test_cli_parsing_tools_call_missing_name() {
    let args = vec!["turbomcp-cli", "tools-call"];

    let result = Cli::try_parse_from(args);
    assert!(result.is_err(), "Should fail without tool name");

    let error = result.unwrap_err();
    let error_str = error.to_string();
    assert!(error_str.contains("required") || error_str.contains("name"));
}

// Test JSON output formatting
#[tokio::test]
async fn test_output_function_edge_cases() {
    let conn = Connection {
        transport: Some(TransportKind::Http),
        command: None,
        url: "test".to_string(),
        auth: None,
        json: true,
    };

    // Test with complex JSON
    let complex_json = json!({
        "tools": [
            {
                "name": "add",
                "description": "Add two numbers",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "a": {"type": "number"},
                        "b": {"type": "number"}
                    },
                    "required": ["a", "b"]
                }
            }
        ],
        "metadata": {
            "version": "1.0.0",
            "capabilities": ["tools"]
        }
    });

    let result = turbomcp_cli::output(&conn, &complex_json);
    assert!(result.is_ok());

    // Test with null value
    let null_value = json!(null);
    let result = turbomcp_cli::output(&conn, &null_value);
    assert!(result.is_ok());

    // Test with empty object
    let empty_obj = json!({});
    let result = turbomcp_cli::output(&conn, &empty_obj);
    assert!(result.is_ok());

    // Test with empty array
    let empty_array = json!([]);
    let result = turbomcp_cli::output(&conn, &empty_array);
    assert!(result.is_ok());
}

// Test non-JSON output
#[tokio::test]
async fn test_output_non_json_mode() {
    let conn = Connection {
        transport: Some(TransportKind::Http),
        command: None,
        url: "test".to_string(),
        auth: None,
        json: false, // non-JSON mode
    };

    let test_data = json!({
        "result": "success",
        "data": [1, 2, 3]
    });

    let result = turbomcp_cli::output(&conn, &test_data);
    assert!(result.is_ok());
}

// Test various URL formats
#[test]
fn test_url_formats() {
    let urls = vec![
        "http://localhost:8080/mcp",
        "https://api.example.com/v1/mcp",
        "http://127.0.0.1:3000/api/mcp",
        "https://subdomain.example.com:8443/path/to/mcp",
        "ws://localhost:8080/mcp",
        "wss://api.example.com/ws/mcp",
    ];

    for url in urls {
        let conn = Connection {
            transport: Some(TransportKind::Http),
            command: None,
            url: url.to_string(),
            auth: None,
            json: false,
        };

        // Test that various URL formats are accepted
        assert_eq!(conn.url, url);
        assert!(!conn.url.is_empty());
    }
}

// Test authentication token formats
#[test]
fn test_auth_token_formats() {
    let tokens = vec![
        "simple_token",
        "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...",
        "api_key_123456789",
        "sk-1234567890abcdef",
        "", // empty token
    ];

    for token in tokens {
        let conn = Connection {
            transport: Some(TransportKind::Http),
            command: None,
            url: "http://localhost:8080/test".to_string(),
            auth: if token.is_empty() {
                None
            } else {
                Some(token.to_string())
            },
            json: false,
        };

        if token.is_empty() {
            assert!(conn.auth.is_none());
        } else {
            assert_eq!(conn.auth.as_ref().unwrap(), token);
        }
    }
}
