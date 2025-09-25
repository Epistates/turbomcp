//! Real tests for helper macros - validates actual behavior, not compilation

use turbomcp_macros::*;
use turbomcp_protocol::types::ContentBlock;

#[test]
fn test_mcp_text_produces_correct_content() {
    let content = mcp_text!("Hello World");

    assert!(matches!(content, ContentBlock::Text(_)));
    if let ContentBlock::Text(text_content) = content {
        assert_eq!(text_content.text, "Hello World");
    }
}

#[test]
fn test_mcp_text_formatting_works() {
    let name = "Alice";
    let age = 30;
    let content = mcp_text!("Name: {}, Age: {}", name, age);

    if let ContentBlock::Text(text_content) = content {
        assert_eq!(text_content.text, "Name: Alice, Age: 30");
    } else {
        panic!("Expected TextContent");
    }
}

#[test]
fn test_mcp_error_creates_proper_error() {
    let error = mcp_error!("Something went wrong");

    // Test that mcp_error! creates a ServerError
    let error_msg = format!("{}", error);
    assert!(error_msg.contains("Something went wrong"));
}

#[test]
fn test_mcp_error_formatting_works() {
    let operation = "database";
    let code = 500;
    let error = mcp_error!("Failed {}: code {}", operation, code);

    let error_msg = format!("{}", error);
    assert!(error_msg.contains("Failed database: code 500"));
}

#[test]
fn test_tool_result_empty_creates_valid_result() {
    let result = tool_result!();

    assert!(result.content.is_empty());
    assert!(!result.is_error.unwrap_or(true));
}

#[test]
fn test_tool_result_with_content() {
    let text_content = mcp_text!("Success");
    let result = tool_result!(content = [text_content]);

    assert_eq!(result.content.len(), 1);
    assert!(!result.is_error.unwrap_or(true));

    if let ContentBlock::Text(text) = &result.content[0] {
        assert_eq!(text.text, "Success");
    } else {
        panic!("Expected TextContent");
    }
}

#[tokio::test]
async fn test_helper_macros_async_compatibility() {
    async fn async_operation() -> String {
        "async result".to_string()
    }

    let result = async_operation().await;
    let content = mcp_text!("Result: {}", result);
    let tool_result = tool_result!(content = [content]);

    assert_eq!(tool_result.content.len(), 1);
    if let ContentBlock::Text(text) = &tool_result.content[0] {
        assert_eq!(text.text, "Result: async result");
    }
}
