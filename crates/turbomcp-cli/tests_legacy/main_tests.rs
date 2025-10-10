//! Tests for the main.rs entry point and run_cli function

use std::env;
use std::process::Command;

#[test]
fn test_main_function_exists() {
    // This is a compilation test that ensures main.rs can be built
    let output = Command::new("cargo")
        .args(["build", "--bin", "turbomcp-cli"])
        .current_dir(env::var("CARGO_MANIFEST_DIR").unwrap())
        .output()
        .expect("Failed to execute cargo build");

    assert!(
        output.status.success(),
        "Build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_cli_help_output() {
    // Test that the CLI binary can show help without error
    let output = Command::new("cargo")
        .args(["run", "--bin", "turbomcp-cli", "--", "--help"])
        .current_dir(env::var("CARGO_MANIFEST_DIR").unwrap())
        .output()
        .expect("Failed to execute CLI help command");

    // Help should succeed (exit code 0)
    assert!(
        output.status.success(),
        "Help command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Command-line interface for interacting with MCP servers"));
    assert!(stdout.contains("tools-list"));
    assert!(stdout.contains("tools-call"));
    assert!(stdout.contains("schema-export"));
}

#[test]
fn test_cli_version_output() {
    // Test that the CLI binary can show version without error
    let output = Command::new("cargo")
        .args(["run", "--bin", "turbomcp-cli", "--", "--version"])
        .current_dir(env::var("CARGO_MANIFEST_DIR").unwrap())
        .output()
        .expect("Failed to execute CLI version command");

    // Version should succeed (exit code 0)
    assert!(
        output.status.success(),
        "Version command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should contain version information (at minimum the version number)
    assert!(
        !stdout.trim().is_empty(),
        "Version output should not be empty"
    );
}

#[test]
fn test_run_cli_function_callable() {
    // Test that the run_cli function is accessible from the library
    // This doesn't actually run it (since it would parse command line args and could hang)
    // but ensures the function is properly exposed and can be called programmatically
    use turbomcp_cli::run_cli;

    // This test just verifies the function exists and is accessible
    // Function existence test - integration tests with actual CLI execution are in separate test functions
    let function_ptr = run_cli as fn();
    assert!(
        !std::ptr::eq(function_ptr as *const (), std::ptr::null()),
        "run_cli function should be accessible"
    );
}

#[test]
fn test_cli_without_args_shows_help() {
    // Test running CLI without subcommand shows help or error appropriately
    let output = Command::new("cargo")
        .args(["run", "--bin", "turbomcp-cli"])
        .current_dir(env::var("CARGO_MANIFEST_DIR").unwrap())
        .output()
        .expect("Failed to execute CLI without args");

    // Should fail with non-zero exit code when no subcommand provided
    assert!(
        !output.status.success(),
        "CLI should fail when no subcommand provided"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    // Should show usage or error message
    assert!(
        stderr.contains("Usage:") || stderr.contains("required") || stderr.contains("subcommand"),
        "Should show usage information when no subcommand provided"
    );
}

#[test]
fn test_invalid_subcommand() {
    // Test running CLI with invalid subcommand
    let output = Command::new("cargo")
        .args(["run", "--bin", "turbomcp-cli", "--", "invalid-command"])
        .current_dir(env::var("CARGO_MANIFEST_DIR").unwrap())
        .output()
        .expect("Failed to execute CLI with invalid command");

    // Should fail with non-zero exit code for invalid subcommand
    assert!(
        !output.status.success(),
        "CLI should fail with invalid subcommand"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    // Should show error about unrecognized subcommand
    assert!(
        stderr.contains("unrecognized subcommand")
            || stderr.contains("invalid-command")
            || stderr.contains("not found"),
        "Should show error about unrecognized subcommand"
    );
}

// Integration test for the actual CLI behavior
#[test]
fn test_tools_list_stdio_error_via_cli() {
    // Test tools-list command with stdio transport (should error)
    let output = Command::new("cargo")
        .args([
            "run",
            "--bin",
            "turbomcp-cli",
            "--",
            "tools-list",
            "--transport",
            "stdio",
        ])
        .current_dir(env::var("CARGO_MANIFEST_DIR").unwrap())
        .output()
        .expect("Failed to execute tools-list command");

    // Should fail since no command is provided for stdio transport
    assert!(
        !output.status.success(),
        "tools-list with stdio should fail without command"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No command specified") || stderr.contains("command"),
        "Should show missing command error: {stderr}"
    );
}

#[test]
fn test_tools_call_with_invalid_json() {
    // Test tools-call command with invalid JSON arguments
    let output = Command::new("cargo")
        .args([
            "run",
            "--bin",
            "turbomcp-cli",
            "--",
            "tools-call",
            "--transport",
            "http",
            "--url",
            "http://localhost:8080/test",
            "--name",
            "test_tool",
            "--arguments",
            "invalid json",
        ])
        .current_dir(env::var("CARGO_MANIFEST_DIR").unwrap())
        .output()
        .expect("Failed to execute tools-call command");

    // Should fail due to invalid JSON
    assert!(
        !output.status.success(),
        "tools-call with invalid JSON should fail"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Invalid JSON arguments") || stderr.contains("JSON"),
        "Should show JSON parsing error"
    );
}

#[test]
fn test_schema_export_stdio_error_via_cli() {
    // Test schema-export command with stdio transport (should error)
    let output = Command::new("cargo")
        .args([
            "run",
            "--bin",
            "turbomcp-cli",
            "--",
            "schema-export",
            "--transport",
            "stdio",
        ])
        .current_dir(env::var("CARGO_MANIFEST_DIR").unwrap())
        .output()
        .expect("Failed to execute schema-export command");

    // Should fail since no command is provided for stdio transport
    assert!(
        !output.status.success(),
        "schema-export with stdio should fail without command"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No command specified") || stderr.contains("command"),
        "Should show missing command error: {stderr}"
    );
}
