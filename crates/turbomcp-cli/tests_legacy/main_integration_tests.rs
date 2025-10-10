//! Integration tests for turbomcp-cli main.rs and core functionality

use clap::Parser;
use serde_json::json;
use std::process::Command;
use turbomcp_cli::{
    Cli, Commands, Connection, TransportKind, cmd_schema_export, cmd_tools_call, cmd_tools_list,
    output,
};

// Test the main function indirectly through the binary
#[test]
fn test_main_binary_exists() {
    // Test that the binary can be built and executed
    let output = Command::new("cargo")
        .args(["build", "--bin", "turbomcp-cli"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output();

    match output {
        Ok(result) => {
            if !result.status.success() {
                let stderr = String::from_utf8_lossy(&result.stderr);
                eprintln!("Build failed: {stderr}");
            }
            // Binary should build successfully
            assert!(result.status.success(), "Binary should build successfully");
        }
        Err(e) => {
            eprintln!("Failed to run cargo build: {e}");
            // If cargo is not available, we'll skip this test
        }
    }
}

#[test]
fn test_main_function_callable() {
    // Test that run_cli() can be called (it will likely fail due to missing args)
    // We'll capture this by setting up a controlled environment

    // Create a minimal CLI structure
    let args = vec!["turbomcp-cli", "--help"];
    let cli_result = Cli::try_parse_from(args);

    // Should fail because --help exits early, but parsing should work
    match cli_result {
        Ok(_) => {
            // Successful parse means we can create the CLI structure
        }
        Err(e) => {
            // Help or version errors are expected and indicate the CLI is working
            let error_str = e.to_string();
            assert!(
                error_str.contains("Usage:")
                    || error_str.contains("help")
                    || error_str.contains("version"),
                "Error should be help-related: {error_str}"
            );
        }
    }
}

// Test CLI parsing functionality
#[test]
fn test_cli_parsing_tools_list() {
    let args = vec![
        "turbomcp-cli",
        "tools-list",
        "--transport",
        "http",
        "--url",
        "http://test.com",
    ];

    let cli = Cli::try_parse_from(args).unwrap();

    match cli.command {
        Commands::ToolsList(conn) => {
            assert_eq!(conn.url, "http://test.com");
            assert!(matches!(conn.transport, Some(TransportKind::Http)));
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
        "http",
        "--url",
        "http://test.com",
        "--name",
        "test_tool",
        "--arguments",
        r#"{"key": "value"}"#,
    ];

    let cli = Cli::try_parse_from(args).unwrap();

    match cli.command {
        Commands::ToolsCall {
            conn,
            name,
            arguments,
        } => {
            assert_eq!(conn.url, "http://test.com");
            assert_eq!(name, "test_tool");
            assert_eq!(arguments, r#"{"key": "value"}"#);
            assert!(matches!(conn.transport, Some(TransportKind::Http)));
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
        "ws",
        "--url",
        "ws://test.com",
        "--json",
    ];

    let cli = Cli::try_parse_from(args).unwrap();

    match cli.command {
        Commands::SchemaExport { conn, .. } => {
            assert_eq!(conn.url, "ws://test.com");
            assert!(matches!(conn.transport, Some(TransportKind::Ws)));
            assert!(conn.json);
        }
        _ => panic!("Expected SchemaExport command"),
    }
}

#[test]
fn test_cli_parsing_with_auth() {
    let args = vec![
        "turbomcp-cli",
        "tools-list",
        "--transport",
        "http",
        "--auth",
        "bearer_token_123",
    ];

    let cli = Cli::try_parse_from(args).unwrap();

    match cli.command {
        Commands::ToolsList(conn) => {
            assert_eq!(conn.auth, Some("bearer_token_123".to_string()));
        }
        _ => panic!("Expected ToolsList command"),
    }
}

#[test]
fn test_cli_parsing_defaults() {
    let args = vec!["turbomcp-cli", "tools-list"];

    let cli = Cli::try_parse_from(args).unwrap();

    match cli.command {
        Commands::ToolsList(conn) => {
            assert!(conn.transport.is_none()); // None for auto-detection
            assert_eq!(conn.url, "http://localhost:8080/mcp");
            assert_eq!(conn.auth, None);
            assert!(!conn.json);
        }
        _ => panic!("Expected ToolsList command"),
    }
}

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
}

// Test output function
#[test]
fn test_output_json_format() {
    let conn = Connection {
        transport: Some(TransportKind::Http),
        command: None,
        url: "http://test.com".to_string(),
        auth: None,
        json: true,
    };

    let value = json!({"test": "data", "number": 42});
    let result = output(&conn, &value);

    assert!(result.is_ok());
}

#[test]
fn test_output_non_json_format() {
    let conn = Connection {
        transport: Some(TransportKind::Http),
        command: None,
        url: "http://test.com".to_string(),
        auth: None,
        json: false,
    };

    let value = json!({"test": "data"});
    let result = output(&conn, &value);

    assert!(result.is_ok());
}

// Test command functions for error handling (they'll fail due to no server, but we test error paths)
#[tokio::test]
async fn test_cmd_tools_list_stdio_error() {
    let conn = Connection {
        transport: Some(TransportKind::Stdio),
        command: None,
        url: "unused".to_string(),
        auth: None,
        json: false,
    };

    let result = cmd_tools_list(conn).await;
    assert!(result.is_err());

    if let Err(e) = result {
        assert!(e.contains("Failed to spawn command") || e.contains("command"));
    }
}

#[tokio::test]
async fn test_cmd_tools_call_stdio_error() {
    let conn = Connection {
        transport: Some(TransportKind::Stdio),
        command: None,
        url: "unused".to_string(),
        auth: None,
        json: false,
    };

    let result = cmd_tools_call(conn, "test_tool".to_string(), "{}".to_string()).await;
    assert!(result.is_err());

    if let Err(e) = result {
        assert!(e.contains("Failed to spawn command") || e.contains("command"));
    }
}

#[tokio::test]
async fn test_cmd_schema_export_stdio_error() {
    let conn = Connection {
        transport: Some(TransportKind::Stdio),
        command: None,
        url: "unused".to_string(),
        auth: None,
        json: false,
    };

    let result = cmd_schema_export(conn, None).await;
    assert!(result.is_err());

    if let Err(e) = result {
        assert!(e.contains("Failed to spawn command") || e.contains("command"));
    }
}

#[tokio::test]
async fn test_cmd_tools_call_invalid_arguments() {
    let conn = Connection {
        transport: Some(TransportKind::Http),
        command: None,
        url: "http://nonexistent.com".to_string(),
        auth: None,
        json: false,
    };

    // This should fail due to invalid JSON arguments before even trying to connect
    let result = cmd_tools_call(conn, "test_tool".to_string(), "invalid_json{".to_string()).await;
    assert!(result.is_err());

    if let Err(e) = result {
        assert!(e.contains("Invalid JSON arguments") || e.contains("JSON"));
    }
}

// Test Connection and TransportKind structures
#[test]
fn test_connection_debug_format() {
    let conn = Connection {
        transport: Some(TransportKind::Http),
        command: None,
        url: "http://test.com".to_string(),
        auth: Some("token".to_string()),
        json: true,
    };

    let debug_str = format!("{conn:?}");
    assert!(debug_str.contains("Connection"));
    assert!(debug_str.contains("Http"));
    assert!(debug_str.contains("http://test.com"));
}

#[test]
fn test_connection_clone() {
    let original = Connection {
        transport: Some(TransportKind::Ws),
        url: "ws://test.com".to_string(),
        command: None,
        auth: None,
        json: false,
    };

    let cloned = original.clone();
    assert_eq!(format!("{original:?}"), format!("{:?}", cloned));
}

#[test]
fn test_transport_kind_debug_format() {
    let stdio = TransportKind::Stdio;
    let http = TransportKind::Http;
    let ws = TransportKind::Ws;

    assert!(format!("{stdio:?}").contains("Stdio"));
    assert!(format!("{http:?}").contains("Http"));
    assert!(format!("{ws:?}").contains("Ws"));
}

#[test]
fn test_transport_kind_clone() {
    let original = TransportKind::Http;
    let cloned = original.clone();

    assert!(matches!(cloned, TransportKind::Http));
}

// Test CLI structure
#[test]
fn test_cli_debug_format() {
    let args = vec!["turbomcp-cli", "tools-list"];
    let cli = Cli::try_parse_from(args).unwrap();

    let debug_str = format!("{cli:?}");
    assert!(debug_str.contains("Cli"));
    assert!(debug_str.contains("command"));
}

#[test]
fn test_commands_enum_variants() {
    // Test that all command variants can be created and debugged
    let conn = Connection {
        transport: Some(TransportKind::Http),
        command: None,
        url: "http://test.com".to_string(),
        auth: None,
        json: false,
    };

    let tools_list = Commands::ToolsList(conn.clone());
    let tools_call = Commands::ToolsCall {
        conn: conn.clone(),
        name: "test".to_string(),
        arguments: "{}".to_string(),
    };
    let schema_export = Commands::SchemaExport { conn, output: None };

    // All should be debuggable
    let debug1 = format!("{tools_list:?}");
    let debug2 = format!("{tools_call:?}");
    let debug3 = format!("{schema_export:?}");

    assert!(debug1.contains("ToolsList"));
    assert!(debug2.contains("ToolsCall"));
    assert!(debug3.contains("SchemaExport"));
}

// Integration test that exercises the main logic paths
#[test]
fn test_main_integration_simulation() {
    // Simulate what main.rs does

    // 1. Parse CLI args
    let args = vec!["turbomcp-cli", "tools-list", "--json"];
    let cli = Cli::try_parse_from(args).unwrap();

    // 2. Verify we have the right command structure
    match cli.command {
        Commands::ToolsList(conn) => {
            assert!(conn.json);

            // 3. The tokio runtime creation and command execution would happen here
            // We can't easily test this without actually starting a server,
            // but we've verified the structure is correct
        }
        _ => panic!("Expected ToolsList command"),
    }
}

// Test error conditions
#[test]
fn test_cli_missing_required_args() {
    // Test commands that require additional arguments
    let args = vec!["turbomcp-cli", "tools-call"];
    let result = Cli::try_parse_from(args);

    // Should fail because tools-call requires a name argument
    assert!(result.is_err());
}

#[test]
fn test_invalid_url_format() {
    // Test that invalid URLs are handled (they should still parse, just fail at runtime)
    let args = vec!["turbomcp-cli", "tools-list", "--url", "not_a_valid_url"];

    let cli = Cli::try_parse_from(args).unwrap();

    match cli.command {
        Commands::ToolsList(conn) => {
            assert_eq!(conn.url, "not_a_valid_url");
            // URL validation happens at runtime, not parse time
        }
        _ => panic!("Expected ToolsList command"),
    }
}

// Test with various argument combinations
#[test]
fn test_comprehensive_argument_combinations() {
    let test_cases = vec![
        (
            vec!["turbomcp-cli", "tools-list"],
            "tools-list with defaults",
        ),
        (
            vec!["turbomcp-cli", "tools-list", "--json"],
            "tools-list with json",
        ),
        (
            vec!["turbomcp-cli", "tools-list", "--transport", "stdio"],
            "tools-list with stdio",
        ),
        (
            vec!["turbomcp-cli", "schema-export", "--transport", "http"],
            "schema-export",
        ),
    ];

    for (args, description) in test_cases {
        let result = Cli::try_parse_from(args);
        assert!(result.is_ok(), "Failed to parse: {description}");
    }
}
