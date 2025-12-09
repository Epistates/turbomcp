//! Fuzz target for JSON-RPC message parsing
//!
//! This fuzzer tests the robustness of JSON-RPC 2.0 message parsing
//! against arbitrary byte sequences, ensuring no panics, memory safety
//! issues, or undefined behavior occur.

#![no_main]

use libfuzzer_sys::fuzz_target;
use serde_json::Value;

fuzz_target!(|data: &[u8]| {
    // Try to parse as UTF-8 string first
    if let Ok(s) = std::str::from_utf8(data) {
        // Attempt to parse as JSON
        if let Ok(json) = serde_json::from_str::<Value>(s) {
            // Try to interpret as JSON-RPC request
            let _ = parse_as_jsonrpc_request(&json);

            // Try to interpret as JSON-RPC response
            let _ = parse_as_jsonrpc_response(&json);

            // Try to interpret as JSON-RPC notification
            let _ = parse_as_jsonrpc_notification(&json);

            // Try batch parsing
            if let Value::Array(arr) = &json {
                for item in arr {
                    let _ = parse_as_jsonrpc_request(item);
                    let _ = parse_as_jsonrpc_response(item);
                }
            }
        }

        // Also try parsing directly as protocol types
        let _ = serde_json::from_str::<turbomcp_protocol::JsonRpcRequest>(s);
        let _ = serde_json::from_str::<turbomcp_protocol::JsonRpcResponse>(s);
        let _ = serde_json::from_str::<turbomcp_protocol::JsonRpcNotification>(s);
    }

    // Also try direct byte parsing (for binary protocol edge cases)
    let _ = serde_json::from_slice::<Value>(data);
    let _ = serde_json::from_slice::<turbomcp_protocol::JsonRpcRequest>(data);
    let _ = serde_json::from_slice::<turbomcp_protocol::JsonRpcResponse>(data);
});

/// Validate JSON-RPC request structure
fn parse_as_jsonrpc_request(json: &Value) -> Option<()> {
    let obj = json.as_object()?;

    // Check jsonrpc version
    let version = obj.get("jsonrpc")?.as_str()?;
    if version != "2.0" {
        return None;
    }

    // Check method
    let _method = obj.get("method")?.as_str()?;

    // ID can be string, number, or null (but must be present for requests)
    let id = obj.get("id")?;
    match id {
        Value::String(_) | Value::Number(_) | Value::Null => {}
        _ => return None,
    }

    // Params are optional but must be object or array if present
    if let Some(params) = obj.get("params") {
        match params {
            Value::Object(_) | Value::Array(_) => {}
            _ => return None,
        }
    }

    Some(())
}

/// Validate JSON-RPC response structure
fn parse_as_jsonrpc_response(json: &Value) -> Option<()> {
    let obj = json.as_object()?;

    // Check jsonrpc version
    let version = obj.get("jsonrpc")?.as_str()?;
    if version != "2.0" {
        return None;
    }

    // ID must be present
    let id = obj.get("id")?;
    match id {
        Value::String(_) | Value::Number(_) | Value::Null => {}
        _ => return None,
    }

    // Must have either result or error, but not both
    let has_result = obj.contains_key("result");
    let has_error = obj.contains_key("error");

    if has_result == has_error {
        return None; // Must have exactly one
    }

    // Validate error structure if present
    if let Some(error) = obj.get("error") {
        let err_obj = error.as_object()?;
        let _code = err_obj.get("code")?.as_i64()?;
        let _message = err_obj.get("message")?.as_str()?;
        // data is optional
    }

    Some(())
}

/// Validate JSON-RPC notification structure
fn parse_as_jsonrpc_notification(json: &Value) -> Option<()> {
    let obj = json.as_object()?;

    // Check jsonrpc version
    let version = obj.get("jsonrpc")?.as_str()?;
    if version != "2.0" {
        return None;
    }

    // Check method
    let _method = obj.get("method")?.as_str()?;

    // Notifications must NOT have an id
    if obj.contains_key("id") {
        return None;
    }

    Some(())
}
