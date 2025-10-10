//! Integration tests for turbomcp-cli

use clap::Parser;
use turbomcp_cli::{Cli, Commands, TransportKind};

#[test]
fn test_cli_basic_parsing() {
    // Test basic command parsing
    let args = vec!["turbomcp-cli", "tools-list"];
    let cli = Cli::try_parse_from(args).expect("Failed to parse CLI args");

    match cli.command {
        Commands::ToolsList(_) => {
            // Success - can parse basic tools-list command
        }
        _ => panic!("Expected ToolsList command"),
    }
}

#[test]
fn test_transport_options() {
    // Test explicit transport
    let args = vec!["turbomcp-cli", "tools-list", "--transport", "stdio"];
    let cli = Cli::try_parse_from(args).expect("Failed to parse CLI args");

    match cli.command {
        Commands::ToolsList(conn) => {
            assert!(matches!(conn.transport, Some(TransportKind::Stdio)));
        }
        _ => panic!("Expected ToolsList command"),
    }
}

#[test]
fn test_command_option() {
    // Test --command option
    let args = vec!["turbomcp-cli", "tools-list", "--command", "./my-server"];
    let cli = Cli::try_parse_from(args).expect("Failed to parse CLI args");

    match cli.command {
        Commands::ToolsList(conn) => {
            assert_eq!(conn.command, Some("./my-server".to_string()));
        }
        _ => panic!("Expected ToolsList command"),
    }
}

#[test]
fn test_schema_export_with_output() {
    // Test schema export with output file
    let args = vec!["turbomcp-cli", "schema-export", "--output", "test.json"];
    let cli = Cli::try_parse_from(args).expect("Failed to parse CLI args");

    match cli.command {
        Commands::SchemaExport { conn: _, output } => {
            assert_eq!(output, Some("test.json".to_string()));
        }
        _ => panic!("Expected SchemaExport command"),
    }
}

#[test]
fn test_tools_call_basic() {
    // Test tools call with name and arguments
    let args = vec![
        "turbomcp-cli",
        "tools-call",
        "--name",
        "test_tool",
        "--arguments",
        r#"{"key": "value"}"#,
    ];
    let cli = Cli::try_parse_from(args).expect("Failed to parse CLI args");

    match cli.command {
        Commands::ToolsCall {
            name, arguments, ..
        } => {
            assert_eq!(name, "test_tool");
            assert_eq!(arguments, r#"{"key": "value"}"#);
        }
        _ => panic!("Expected ToolsCall command"),
    }
}

#[test]
fn test_connection_defaults() {
    // Test default values
    let args = vec!["turbomcp-cli", "tools-list"];
    let cli = Cli::try_parse_from(args).expect("Failed to parse CLI args");

    match cli.command {
        Commands::ToolsList(conn) => {
            assert_eq!(conn.url, "http://localhost:8080/mcp");
            assert_eq!(conn.transport, None); // Should be None, allowing auto-detection
            assert_eq!(conn.command, None);
            assert_eq!(conn.auth, None);
            assert!(!conn.json);
        }
        _ => panic!("Expected ToolsList command"),
    }
}
