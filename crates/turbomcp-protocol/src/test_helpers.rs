//! Test utilities for turbomcp-protocol
//!
//! This module provides shared test helpers used throughout the crate's tests.
//! Following the pattern from axum and tokio, these utilities are public to
//! allow downstream crates to use them in their tests.
//!
//! ## Organization
//!
//! All test fixtures and helpers are in this single module for simplicity.
//! As the test suite grows, this can be split into submodules if needed.
//!
//! ## Usage
//!
//! ```rust
//! #[cfg(test)]
//! mod tests {
//!     use super::*;
//!     use crate::test_helpers::*;
//!
//!     #[test]
//!     fn my_test() {
//!         let request = test_request();
//!         assert_valid(&result);
//!     }
//! }
//! ```

use crate::jsonrpc::*;
use crate::types::*;
use crate::validation::*;

// ========== JSON-RPC Request Fixtures ==========

/// Create a standard test JSON-RPC request
pub fn test_request() -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        method: "tools/list".to_string(),
        params: None,
        id: RequestId::String("test-123".to_string()),
    }
}

/// Create a valid tool for testing
pub fn test_tool() -> Tool {
    Tool {
        name: "test_tool".to_string(),
        title: Some("Test Tool".to_string()),
        description: Some("A test tool for validation".to_string()),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: None,
            required: None,
            additional_properties: None,
        },
        output_schema: None,
        annotations: None,
        #[cfg(feature = "mcp-icons")]
        icons: None,
        meta: None,
    }
}

/// Create a valid prompt for testing
pub fn test_prompt() -> Prompt {
    Prompt {
        name: "test_prompt".to_string(),
        title: Some("Test Prompt".to_string()),
        description: Some("A test prompt".to_string()),
        arguments: None,
        meta: None,
    }
}

/// Create a prompt argument for testing
pub fn test_prompt_argument(name: &str) -> PromptArgument {
    PromptArgument {
        name: name.to_string(),
        title: Some(format!("Argument {name}")),
        description: Some(format!("Description for {name}")),
        required: Some(true),
    }
}

/// Create a valid resource for testing
pub fn test_resource() -> Resource {
    Resource {
        name: "test_resource".to_string(),
        title: Some("Test Resource".to_string()),
        uri: "file://test/resource.txt".to_string(),
        description: Some("A test resource".to_string()),
        mime_type: Some("text/plain".to_string()),
        annotations: None,
        size: Some(1024),
        meta: None,
    }
}

/// Create a valid initialize request for testing
pub fn test_initialize_request() -> InitializeRequest {
    InitializeRequest {
        protocol_version: "2025-06-18".to_string(),
        capabilities: ClientCapabilities::default(),
        client_info: Implementation {
            name: "test-client".to_string(),
            title: Some("Test Client".to_string()),
            version: "1.0.0".to_string(),
        },
        _meta: None,
    }
}

// ========== Validation Assertions ==========

/// Assert that validation passed without warnings
#[allow(dead_code)]
pub fn assert_valid(result: &ValidationResult) {
    assert!(
        result.is_valid(),
        "Expected validation to pass, but got errors: {:?}",
        result.errors()
    );
}

/// Assert that validation failed
#[allow(dead_code)]
pub fn assert_invalid(result: &ValidationResult) {
    assert!(
        result.is_invalid(),
        "Expected validation to fail, but it passed"
    );
}
