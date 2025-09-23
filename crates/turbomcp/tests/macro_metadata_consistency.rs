//! Test for macro metadata API consistency across tool, prompt, and resource macros
//!
//! This test ensures that all macro types provide consistent public metadata
//! access functions for testing and integration purposes.

use turbomcp::McpError;
use turbomcp_macros::*;

// Test struct for macro applications
struct TestStruct;

#[allow(dead_code)] // Test methods used only for macro metadata testing
impl TestStruct {
    // Test tool function with macro
    #[tool("Test tool with parameter")]
    async fn test_tool(&self, param: String) -> Result<String, McpError> {
        Ok(format!("Processed: {}", param))
    }

    // Test prompt function with macro
    #[prompt("Test prompt description")]
    async fn test_prompt(&self) -> Result<String, McpError> {
        Ok("Prompt result".to_string())
    }

    // Test resource function with macro
    #[resource("resource://test/{id}")]
    async fn test_resource(&self) -> Result<String, McpError> {
        Ok("Resource content".to_string())
    }
}

#[tokio::test]
async fn test_macro_metadata_api_consistency() {
    // Test that all macros provide public metadata access functions

    // Tool metadata - should return (name, description, schema)
    let (tool_name, tool_desc, tool_schema) = TestStruct::test_tool_metadata();
    assert_eq!(tool_name, "test_tool");
    assert_eq!(tool_desc, "Test tool with parameter");
    assert!(!tool_schema.is_null());

    // Prompt metadata - should return (name, description, arguments_schema, tags)
    let (prompt_name, prompt_desc, prompt_args, prompt_tags) = TestStruct::test_prompt_metadata();
    assert_eq!(prompt_name, "test_prompt");
    assert_eq!(prompt_desc, "Test prompt description");
    assert!(prompt_args.is_empty()); // No parameters
    assert_eq!(prompt_tags, Vec::<String>::new());

    // Resource metadata - should return (uri_template, name, title, description, mime_type, tags)
    let (resource_uri, resource_name, resource_title, resource_desc, resource_mime, resource_tags) =
        TestStruct::test_resource_metadata();
    assert_eq!(resource_uri, "resource://test/{id}");
    assert!(!resource_name.is_empty());
    assert!(!resource_title.is_empty());
    assert!(!resource_desc.is_empty());
    assert!(!resource_mime.is_empty());
    assert_eq!(resource_tags, Vec::<String>::new());
}

#[test]
fn test_metadata_functions_exist_at_compile_time() {
    // This test verifies the functions exist at compile time
    // If any are missing, this won't compile

    let _ = TestStruct::test_tool_metadata;
    let _ = TestStruct::test_prompt_metadata;
    let _ = TestStruct::test_resource_metadata;
}

#[tokio::test]
async fn test_consistent_naming_pattern() {
    // All metadata functions should follow the pattern: {function_name}_metadata

    // The functions should be accessible as expected
    let tool_meta = TestStruct::test_tool_metadata();
    let prompt_meta = TestStruct::test_prompt_metadata();
    let resource_meta = TestStruct::test_resource_metadata();

    // Names should match the original function names
    assert_eq!(tool_meta.0, "test_tool");
    assert_eq!(prompt_meta.0, "test_prompt");
    assert_eq!(resource_meta.0, "resource://test/{id}"); // Resource meta now starts with URI
}
