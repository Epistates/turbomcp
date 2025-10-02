//! Common utilities shared across transport implementations

use serde_json::json;

/// Extract tool schemas from a tools/list JSON-RPC response
///
/// Transforms the response from `tools/list` into a simplified schema format
/// containing just the tool names and their input schemas.
///
/// # Arguments
/// * `response` - JSON-RPC response containing tools list
///
/// # Returns
/// * `Ok(Value)` - Transformed schemas or original response if transformation fails
/// * `Err(String)` - Never returns error, always succeeds
pub fn extract_schemas(response: serde_json::Value) -> Result<serde_json::Value, String> {
    if let Some(result) = response.get("result")
        && let Some(tools) = result.get("tools").and_then(|t| t.as_array())
    {
        let mut out = Vec::new();
        for tool in tools {
            let name = tool
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown");
            let schema = tool.get("inputSchema").cloned().unwrap_or(json!({}));
            out.push(json!({"name": name, "schema": schema}));
        }
        return Ok(json!({"schemas": out}));
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_schemas_with_tools() {
        let response = json!({
            "result": {
                "tools": [
                    {
                        "name": "add",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "a": {"type": "number"},
                                "b": {"type": "number"}
                            }
                        }
                    }
                ]
            }
        });

        let result = extract_schemas(response).unwrap();
        assert_eq!(result["schemas"][0]["name"], "add");
        assert!(result["schemas"][0]["schema"].is_object());
    }

    #[test]
    fn test_extract_schemas_no_tools() {
        let response = json!({"result": {}});
        let result = extract_schemas(response.clone()).unwrap();
        assert_eq!(result, response);
    }
}
