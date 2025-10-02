//! HTTP transport implementation for MCP servers

use crate::cli::Connection;
use serde_json::json;

/// List tools from HTTP MCP server
pub async fn list_tools(conn: &Connection) -> Result<serde_json::Value, String> {
    let req = json!({"jsonrpc":"2.0","id":"1","method":"tools/list"});
    post(conn, req).await
}

/// Call a tool on HTTP MCP server
pub async fn call_tool(
    conn: &Connection,
    name: String,
    arguments: String,
) -> Result<serde_json::Value, String> {
    let args: serde_json::Value =
        serde_json::from_str(&arguments).map_err(|e| format!("Invalid JSON arguments: {e}"))?;
    let req = json!({
        "jsonrpc":"2.0","id":"1","method":"tools/call",
        "params": {"name": name, "arguments": args}
    });
    post(conn, req).await
}

/// Get tool schemas from HTTP MCP server
pub async fn get_schemas(conn: &Connection) -> Result<serde_json::Value, String> {
    // List, then return each tool's inputSchema
    let req = json!({"jsonrpc":"2.0","id":"1","method":"tools/list"});
    let res = post(conn, req).await?;
    super::common::extract_schemas(res)
}

/// Send HTTP POST request with JSON-RPC payload
async fn post(conn: &Connection, body: serde_json::Value) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::new();
    let mut req = client.post(&conn.url).json(&body);
    if let Some(auth) = &conn.auth {
        req = req.bearer_auth(auth);
    }
    let res = req.send().await.map_err(|e| e.to_string())?;
    let status = res.status();
    let text = res.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("HTTP {status}: {text}"));
    }
    serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))
}
