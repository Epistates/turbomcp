//! Fuzz target for MCP message validation
//!
//! This fuzzer tests the protocol's message validation logic against
//! malformed, edge-case, and adversarial inputs.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use serde_json::{json, Value};

/// Structured input for message fuzzing
#[derive(Debug, Arbitrary)]
struct MessageFuzzInput {
    // JSON-RPC fields
    jsonrpc_version: u8,    // 0 = "2.0", 1 = "1.0", 2 = missing, 3 = invalid
    id_type: u8,            // 0 = int, 1 = string, 2 = null, 3 = missing, 4 = invalid
    id_value: i64,
    id_string: String,
    method_present: bool,
    method_value: String,

    // Params
    params_type: u8,        // 0 = object, 1 = array, 2 = missing, 3 = invalid

    // Result/Error
    has_result: bool,
    has_error: bool,
    error_code: i32,
    error_message: String,

    // Nesting depth attack
    nesting_depth: u8,
}

fuzz_target!(|data: &[u8]| {
    // Strategy 1: Raw byte parsing
    if let Ok(s) = std::str::from_utf8(data) {
        // Try parsing various MCP message types
        let _ = serde_json::from_str::<turbomcp_protocol::JsonRpcRequest>(s);
        let _ = serde_json::from_str::<turbomcp_protocol::JsonRpcResponse>(s);
        let _ = serde_json::from_str::<turbomcp_protocol::JsonRpcNotification>(s);

        // Try protocol-specific types
        let _ = serde_json::from_str::<turbomcp_protocol::types::InitializeRequest>(s);
        let _ = serde_json::from_str::<turbomcp_protocol::types::InitializeResult>(s);
        let _ = serde_json::from_str::<turbomcp_protocol::types::CallToolRequest>(s);
        let _ = serde_json::from_str::<turbomcp_protocol::types::CallToolResult>(s);
        let _ = serde_json::from_str::<turbomcp_protocol::types::GetPromptRequest>(s);
        let _ = serde_json::from_str::<turbomcp_protocol::types::GetPromptResult>(s);
        let _ = serde_json::from_str::<turbomcp_protocol::types::ReadResourceRequest>(s);
        let _ = serde_json::from_str::<turbomcp_protocol::types::ReadResourceResult>(s);

        // Try content types
        let _ = serde_json::from_str::<turbomcp_protocol::types::Content>(s);
        let _ = serde_json::from_str::<turbomcp_protocol::types::TextContent>(s);
        let _ = serde_json::from_str::<turbomcp_protocol::types::ImageContent>(s);
        let _ = serde_json::from_str::<turbomcp_protocol::types::AudioContent>(s);
        let _ = serde_json::from_str::<turbomcp_protocol::types::ResourceContent>(s);
    }

    // Strategy 2: Structured message generation
    if let Ok(input) = MessageFuzzInput::arbitrary(&mut arbitrary::Unstructured::new(data)) {
        let message = build_fuzz_message(&input);

        // Serialize and try to parse
        if let Ok(json_str) = serde_json::to_string(&message) {
            let _ = serde_json::from_str::<turbomcp_protocol::JsonRpcRequest>(&json_str);
            let _ = serde_json::from_str::<turbomcp_protocol::JsonRpcResponse>(&json_str);
            let _ = serde_json::from_str::<turbomcp_protocol::JsonRpcNotification>(&json_str);
        }
    }
});

fn build_fuzz_message(input: &MessageFuzzInput) -> Value {
    let mut msg = serde_json::Map::new();

    // jsonrpc version
    match input.jsonrpc_version % 4 {
        0 => { msg.insert("jsonrpc".to_string(), json!("2.0")); }
        1 => { msg.insert("jsonrpc".to_string(), json!("1.0")); }
        2 => { /* missing */ }
        _ => { msg.insert("jsonrpc".to_string(), json!(123)); }
    }

    // ID field
    match input.id_type % 5 {
        0 => { msg.insert("id".to_string(), json!(input.id_value)); }
        1 => { msg.insert("id".to_string(), json!(truncate(&input.id_string, 64))); }
        2 => { msg.insert("id".to_string(), Value::Null); }
        3 => { /* missing */ }
        _ => { msg.insert("id".to_string(), json!({"invalid": "id"})); }
    }

    // Method
    if input.method_present {
        msg.insert("method".to_string(), json!(truncate(&input.method_value, 128)));
    }

    // Params
    match input.params_type % 4 {
        0 => { msg.insert("params".to_string(), json!({"key": "value"})); }
        1 => { msg.insert("params".to_string(), json!(["item1", "item2"])); }
        2 => { /* missing */ }
        _ => { msg.insert("params".to_string(), json!("invalid_params")); }
    }

    // Result
    if input.has_result {
        // Create nested structure based on depth (limited to prevent DoS)
        let depth = (input.nesting_depth % 10) as usize;
        msg.insert("result".to_string(), build_nested_value(depth));
    }

    // Error
    if input.has_error {
        msg.insert("error".to_string(), json!({
            "code": input.error_code,
            "message": truncate(&input.error_message, 256),
            "data": null
        }));
    }

    Value::Object(msg)
}

fn build_nested_value(depth: usize) -> Value {
    if depth == 0 {
        json!("leaf")
    } else {
        json!({
            "nested": build_nested_value(depth - 1)
        })
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    s.chars().take(max_len).collect()
}
