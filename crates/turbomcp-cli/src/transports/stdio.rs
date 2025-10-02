//! Standard input/output (STDIO) transport implementation for MCP servers

use crate::cli::Connection;
use serde_json::json;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

/// List tools from STDIO MCP server
pub async fn list_tools(conn: &Connection) -> Result<serde_json::Value, String> {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list",
        "params": {}
    });

    send_request(conn, request).await
}

/// Call a tool on STDIO MCP server
pub async fn call_tool(
    conn: &Connection,
    name: String,
    arguments: String,
) -> Result<serde_json::Value, String> {
    let args: serde_json::Value =
        serde_json::from_str(&arguments).map_err(|e| format!("Invalid JSON arguments: {e}"))?;

    let request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": name,
            "arguments": args
        }
    });

    send_request(conn, request).await
}

/// Get tool schemas from STDIO MCP server
pub async fn get_schemas(conn: &Connection) -> Result<serde_json::Value, String> {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/list",
        "params": {}
    });

    let response = send_request(conn, request).await?;
    super::common::extract_schemas(response)
}

/// Send a JSON-RPC request to STDIO process and receive response
async fn send_request(
    conn: &Connection,
    request: serde_json::Value,
) -> Result<serde_json::Value, String> {
    // Use --command option if provided, otherwise use --url
    let command_str = conn.command.as_deref().unwrap_or(&conn.url);
    let mut parts = command_str.split_whitespace();
    let command = parts
        .next()
        .ok_or("No command specified for STDIO transport")?;
    let args: Vec<&str> = parts.collect();

    let mut child = Command::new(command)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn command '{command}': {e}"))?;

    // Send request
    let stdin = child.stdin.as_mut().ok_or("Failed to get stdin handle")?;
    let request_str =
        serde_json::to_string(&request).map_err(|e| format!("Failed to serialize request: {e}"))?;
    writeln!(stdin, "{request_str}").map_err(|e| format!("Failed to write request: {e}"))?;

    // Read response from stdout while discarding stderr
    let stdout = child.stdout.take().ok_or("Failed to get stdout handle")?;
    let mut reader = BufReader::new(stdout);
    let mut response_line = String::new();

    // Read lines until we get valid JSON (ignore log lines)
    loop {
        response_line.clear();
        let bytes_read = reader
            .read_line(&mut response_line)
            .map_err(|e| format!("Failed to read response: {e}"))?;

        if bytes_read == 0 {
            return Err("No JSON response received from server".to_string());
        }

        // Try to parse as JSON - if it works, we found our response
        if serde_json::from_str::<serde_json::Value>(&response_line).is_ok() {
            break;
        }

        // If line starts with '{' it might be JSON, try it anyway
        if response_line.trim().starts_with('{') {
            break;
        }

        // Otherwise it's probably a log line, continue reading
    }

    // Wait for process to complete
    let output = child
        .wait()
        .map_err(|e| format!("Process execution failed: {e}"))?;

    if !output.success() {
        return Err(format!(
            "Command failed with exit code: {}",
            output.code().unwrap_or(-1)
        ));
    }

    // Parse JSON response
    serde_json::from_str(&response_line).map_err(|e| format!("Invalid JSON response: {e}"))
}
