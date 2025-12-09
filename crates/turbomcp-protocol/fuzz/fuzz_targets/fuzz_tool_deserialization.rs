//! Fuzz target for Tool type deserialization
//!
//! This fuzzer tests the robustness of Tool, ToolExecution, and related
//! type deserialization against arbitrary JSON inputs.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use std::collections::HashMap;
use turbomcp_protocol::types::*;

/// Arbitrary input for tool fuzzing with structured generation
#[derive(Debug, Arbitrary)]
struct ToolFuzzInput {
    name: String,
    title: Option<String>,
    description: Option<String>,
    has_execution: bool,
    task_support_mode: u8, // 0-3 maps to enum variants
    has_input_schema: bool,
    schema_type: u8,
    has_annotations: bool,
}

fuzz_target!(|data: &[u8]| {
    // Strategy 1: Try to parse arbitrary bytes as JSON Tool
    if let Ok(s) = std::str::from_utf8(data) {
        // Try direct deserialization
        let _ = serde_json::from_str::<Tool>(s);
        let _ = serde_json::from_str::<ToolExecution>(s);
        let _ = serde_json::from_str::<ToolInputSchema>(s);
        let _ = serde_json::from_str::<ToolAnnotations>(s);
        let _ = serde_json::from_str::<TaskSupportMode>(s);

        // Try tools/list response
        let _ = serde_json::from_str::<ListToolsResult>(s);

        // Try tools/call request
        let _ = serde_json::from_str::<CallToolRequest>(s);
        let _ = serde_json::from_str::<CallToolResult>(s);
    }

    // Strategy 2: Structured fuzzing with Arbitrary
    if let Ok(input) = ToolFuzzInput::arbitrary(&mut arbitrary::Unstructured::new(data)) {
        // Build a tool from structured input
        // TaskSupportMode variants: Forbidden (default), Optional, Required
        let task_support = match input.task_support_mode % 4 {
            0 => Some(TaskSupportMode::Forbidden),
            1 => Some(TaskSupportMode::Optional),
            2 => Some(TaskSupportMode::Required),
            _ => None,
        };

        let execution = if input.has_execution {
            Some(ToolExecution { task_support })
        } else {
            None
        };

        let input_schema = if input.has_input_schema {
            match input.schema_type % 3 {
                0 => ToolInputSchema::empty(),
                1 => ToolInputSchema::with_properties(
                    [("test".to_string(), serde_json::json!({"type": "string"}))]
                        .into_iter()
                        .collect(),
                ),
                _ => ToolInputSchema::empty(),
            }
        } else {
            ToolInputSchema::empty()
        };

        let annotations = if input.has_annotations {
            Some(ToolAnnotations {
                title: None,
                audience: Some(vec!["assistant".to_string()]),
                priority: Some(0.5),
                destructive_hint: Some(false),
                idempotent_hint: Some(true),
                open_world_hint: Some(false),
                read_only_hint: Some(true),
                task_hint: None,
                custom: HashMap::new(),
            })
        } else {
            None
        };

        let tool = Tool {
            name: sanitize_name(&input.name),
            title: input.title,
            description: input.description,
            input_schema,
            output_schema: None,
            execution,
            annotations,
            meta: None,
        };

        // Test serialization roundtrip
        if let Ok(json) = serde_json::to_string(&tool) {
            let _ = serde_json::from_str::<Tool>(&json);
        }

        // Test JSON value conversion
        if let Ok(value) = serde_json::to_value(&tool) {
            let _ = serde_json::from_value::<Tool>(value);
        }
    }
});

/// Sanitize name to be valid (non-empty, reasonable length)
fn sanitize_name(name: &str) -> String {
    if name.is_empty() {
        "fuzz_tool".to_string()
    } else {
        // Limit length to prevent memory exhaustion
        name.chars().take(256).collect()
    }
}
