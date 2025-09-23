//! Comprehensive integration tests for TurboMCP
//!
//! This file consolidates all integration testing patterns:
//! - Schema validation and generation testing
//! - Concurrent operation validation
//! - Performance and stress testing
//! - Real-world scenario validation

use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicI32, Ordering},
};
use tokio::time::{Duration, timeout};
use turbomcp::prelude::*;

/// Comprehensive integration test server that combines all testing patterns
/// Previously split across BasicTestServer and SchemaValidationServer
#[derive(Clone)]
pub struct IntegrationTestServer {
    counter: Arc<AtomicI32>,
}

impl Default for IntegrationTestServer {
    fn default() -> Self {
        Self::new()
    }
}

impl IntegrationTestServer {
    pub fn new() -> Self {
        Self {
            counter: Arc::new(AtomicI32::new(0)),
        }
    }
}

#[server(name = "integration-comprehensive", version = "1.0.0")]
impl IntegrationTestServer {
    /// Comprehensive parameter validation test covering all JSON Schema types
    /// Critical for MCP schema generation validation
    #[tool("Validates all parameter types and schema generation")]
    async fn comprehensive_params(
        &self,
        required_string: String,
        required_int: i32,
        optional_bool: Option<bool>,
        optional_float: Option<f64>,
        array_param: Vec<String>,
        nested_object: serde_json::Value,
    ) -> McpResult<String> {
        let result = json!({
            "required_string": required_string,
            "required_int": required_int,
            "optional_bool": optional_bool,
            "optional_float": optional_float,
            "array_param": array_param,
            "nested_object": nested_object
        });
        Ok(result.to_string())
    }

    /// Counter increment with concurrent access testing
    #[tool("Increment counter - tests concurrent access")]
    async fn increment_counter(&self) -> McpResult<i32> {
        let new_value = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(new_value)
    }

    /// Counter reset for test isolation
    #[tool("Reset counter to zero")]
    async fn reset_counter(&self) -> McpResult<i32> {
        self.counter.store(0, Ordering::SeqCst);
        Ok(0)
    }

    /// Performance test tool with configurable delay
    #[tool("Performance test with configurable delay")]
    async fn delayed_response(&self, delay_ms: u64) -> McpResult<String> {
        if delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
        Ok(format!("Delayed response after {}ms", delay_ms))
    }

    /// Error handling validation
    #[tool("Test error handling and propagation")]
    async fn test_error(&self, should_error: bool) -> McpResult<String> {
        if should_error {
            Err(McpError::InvalidRequest(
                "Intentional test error".to_string(),
            ))
        } else {
            Ok("Success".to_string())
        }
    }

    /// Complex nested object validation
    #[tool("Complex nested object parameter validation")]
    async fn complex_nested(&self, config: serde_json::Value) -> McpResult<String> {
        // Validate complex nested structure
        if !config.is_object() {
            return Err(McpError::invalid_request("Expected object".to_string()));
        }

        Ok(format!("Processed complex config: {}", config))
    }

    /// Batch operation simulation for concurrent testing
    #[tool("Batch operation simulation")]
    async fn batch_operation(&self, items: Vec<String>) -> McpResult<Vec<String>> {
        let mut results = Vec::new();
        for item in items {
            results.push(format!("processed: {}", item));
        }
        Ok(results)
    }

    /// Dynamic content prompt with parameter validation
    #[prompt("Dynamic content with parameters")]
    async fn dynamic_content(
        &self,
        topic: String,
        complexity: Option<String>,
    ) -> McpResult<String> {
        let level = complexity.unwrap_or_else(|| "medium".to_string());
        Ok(format!("Generate {} content about: {}", level, topic))
    }

    /// Performance metrics prompt
    #[prompt("Performance metrics reporting")]
    async fn performance_metrics(&self) -> McpResult<String> {
        let counter_value = self.counter.load(Ordering::SeqCst);
        Ok(format!(
            "Current system metrics - Counter: {}",
            counter_value
        ))
    }

    /// Parameterized resource with URI template validation
    #[resource("docs://content/{id}")]
    async fn dynamic_docs(&self, uri: String) -> McpResult<String> {
        // Extract ID from URI - simple pattern for test
        let id = uri.strip_prefix("docs://content/").unwrap_or("unknown");
        Ok(format!("Documentation content for ID: {}", id))
    }

    /// Static resource for baseline testing
    #[resource("test://static")]
    async fn static_resource(&self, _uri: String) -> McpResult<String> {
        Ok("Static test content".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::task;

    /// Test schema generation for comprehensive parameter validation
    /// This is critical - it caught the schema bug in the original implementation
    #[tokio::test]
    async fn test_comprehensive_schema_generation() {
        let _server = IntegrationTestServer::new();
        let tools_metadata = IntegrationTestServer::get_tools_metadata();

        // Find the comprehensive_params tool
        let comprehensive_tool = tools_metadata
            .iter()
            .find(|(name, _, _)| name == "comprehensive_params")
            .expect("comprehensive_params tool should exist");

        // Validate schema structure - this catches schema generation bugs
        let schema = &comprehensive_tool.2;
        assert_eq!(schema.get("type").unwrap(), "object");

        let properties = schema.get("properties").unwrap().as_object().unwrap();
        let required = schema.get("required").unwrap().as_array().unwrap();

        // Validate required parameters are correctly marked
        assert!(required.contains(&json!("required_string")));
        assert!(required.contains(&json!("required_int")));
        assert!(!required.iter().any(|v| v == "optional_bool"));
        assert!(!required.iter().any(|v| v == "optional_float"));

        // Validate parameter types
        assert_eq!(properties["required_string"]["type"], "string");
        assert_eq!(properties["required_int"]["type"], "integer");
        assert_eq!(properties["optional_bool"]["type"], "boolean");
        assert_eq!(properties["optional_float"]["type"], "number");
        assert_eq!(properties["array_param"]["type"], "array");
        assert!(
            properties["nested_object"]["type"] == "object"
                || properties.get("nested_object").is_some()
        ); // Accept any type for Value
    }

    /// Test resource parameter extraction - ensures URI template parsing works
    #[tokio::test]
    async fn test_resource_parameter_extraction() {
        let server = IntegrationTestServer::new();
        let resources_metadata = IntegrationTestServer::get_resources_metadata();

        let parameterized_resource = resources_metadata
            .iter()
            .find(|(uri, _, _)| uri.contains("{id}"))
            .expect("Should have parameterized resource");

        // This validates that URI template parsing extracts parameters correctly
        // The first field is the URI template (MCP spec: uri is required)
        assert!(parameterized_resource.0.contains("{id}"));

        // Validate resource can be called with extracted parameter
        let content = server
            .dynamic_docs("docs://content/test123".to_string())
            .await
            .unwrap();
        assert!(content.contains("test123"));
    }

    /// Test concurrent operations - validates thread safety
    #[tokio::test]
    async fn test_concurrent_counter_operations() {
        let server = IntegrationTestServer::new();

        // Reset counter to ensure test isolation
        server.reset_counter().await.unwrap();

        // Spawn multiple concurrent increment operations
        let mut handles = Vec::new();
        for _ in 0..10 {
            let server_clone = server.clone();
            handles.push(task::spawn(async move {
                server_clone.increment_counter().await.unwrap()
            }));
        }

        // Wait for all operations to complete
        let mut results = Vec::new();
        for handle in handles {
            results.push(handle.await.unwrap());
        }

        // Validate all operations completed successfully
        assert_eq!(results.len(), 10);

        // Final counter should be 10
        let final_count = server.counter.load(Ordering::SeqCst);
        assert_eq!(final_count, 10);
    }

    /// Test error handling propagation
    #[tokio::test]
    async fn test_error_handling() {
        let server = IntegrationTestServer::new();

        // Test successful case
        let success_result = server.test_error(false).await;
        assert!(success_result.is_ok());
        assert_eq!(success_result.unwrap(), "Success");

        // Test error case
        let error_result = server.test_error(true).await;
        assert!(error_result.is_err());

        match error_result.unwrap_err() {
            McpError::InvalidRequest(msg) => {
                assert_eq!(msg, "Intentional test error");
            }
            _ => panic!("Expected InvalidRequest error"),
        }
    }

    /// Performance test with timeout validation
    #[tokio::test]
    async fn test_performance_with_timeout() {
        let server = IntegrationTestServer::new();

        // Test fast response (should complete quickly)
        let fast_result = timeout(Duration::from_millis(100), server.delayed_response(10)).await;
        assert!(fast_result.is_ok());
        assert!(fast_result.unwrap().is_ok());

        // Test slow response with appropriate timeout
        let slow_result = timeout(Duration::from_millis(150), server.delayed_response(100)).await;
        assert!(slow_result.is_ok());
        assert!(slow_result.unwrap().is_ok());
    }

    /// Test batch operations for concurrent processing
    #[tokio::test]
    async fn test_batch_processing() {
        let server = IntegrationTestServer::new();

        let items = vec![
            "item1".to_string(),
            "item2".to_string(),
            "item3".to_string(),
        ];

        let results = server.batch_operation(items.clone()).await.unwrap();

        assert_eq!(results.len(), 3);
        for (i, result) in results.iter().enumerate() {
            assert_eq!(result, &format!("processed: {}", items[i]));
        }
    }

    /// Test complex nested parameter validation
    #[tokio::test]
    async fn test_complex_nested_validation() {
        let server = IntegrationTestServer::new();

        // Test valid complex object
        let valid_config = json!({
            "database": {
                "host": "localhost",
                "port": 5432
            },
            "features": ["auth", "logging"],
            "timeout": 30.5
        });

        let result = server.complex_nested(valid_config).await;
        assert!(result.is_ok());

        // Test invalid input (non-object)
        let invalid_config = json!("not an object");
        let error_result = server.complex_nested(invalid_config).await;
        assert!(error_result.is_err());
    }

    /// Test prompt functionality and parameter handling
    #[tokio::test]
    async fn test_prompt_functionality() {
        let server = IntegrationTestServer::new();

        // Test prompt with optional parameter
        let result1 = server
            .dynamic_content("AI systems".to_string(), Some("advanced".to_string()))
            .await;
        assert!(result1.is_ok());
        let content1 = result1.unwrap();
        assert!(content1.contains("advanced"));
        assert!(content1.contains("AI systems"));

        // Test prompt without optional parameter (should use default)
        let result2 = server.dynamic_content("testing".to_string(), None).await;
        assert!(result2.is_ok());
        let content2 = result2.unwrap();
        assert!(content2.contains("medium"));
        assert!(content2.contains("testing"));

        // Test metrics prompt (no parameters)
        let metrics_result = server.performance_metrics().await;
        assert!(metrics_result.is_ok());
        assert!(metrics_result.unwrap().contains("Counter"));
    }

    /// Test resource functionality
    #[tokio::test]
    async fn test_resource_functionality() {
        let server = IntegrationTestServer::new();

        // Test parameterized resource
        let dynamic_result = server
            .dynamic_docs("docs://content/test-doc-123".to_string())
            .await;
        assert!(dynamic_result.is_ok());
        let content = dynamic_result.unwrap();
        assert!(content.contains("test-doc-123"));

        // Test static resource
        let static_result = server.static_resource("test://static".to_string()).await;
        assert!(static_result.is_ok());
        let content = static_result.unwrap();
        assert_eq!(content, "Static test content");
    }

    /// Stress test with multiple concurrent operations
    #[tokio::test]
    async fn test_stress_concurrent_operations() {
        let server = IntegrationTestServer::new();
        server.reset_counter().await.unwrap();

        let mut handles = Vec::new();

        // Mix different types of operations
        for i in 0..20 {
            let server_clone = server.clone();
            handles.push(task::spawn(async move {
                match i % 4 {
                    0 => server_clone.increment_counter().await.map(|_| ()),
                    1 => server_clone.delayed_response(1).await.map(|_| ()),
                    2 => server_clone
                        .batch_operation(vec!["test".to_string()])
                        .await
                        .map(|_| ()),
                    _ => server_clone.test_error(false).await.map(|_| ()),
                }
            }));
        }

        // Wait for all operations
        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok());
        }

        // Validate counter was incremented by the increment operations (5 times)
        let final_count = server.counter.load(Ordering::SeqCst);
        assert_eq!(final_count, 5);
    }

    /// Test metadata consistency across different handler types
    #[tokio::test]
    async fn test_metadata_consistency() {
        let _server = IntegrationTestServer::new();

        // Get all metadata
        let tools = IntegrationTestServer::get_tools_metadata();
        let prompts = IntegrationTestServer::get_prompts_metadata();
        let resources = IntegrationTestServer::get_resources_metadata();

        // Validate tools metadata
        assert!(!tools.is_empty());
        for (name, description, input_schema) in &tools {
            assert!(!name.is_empty());
            assert!(!description.is_empty());
            assert!(input_schema.get("type").is_some());
        }

        // Validate prompts metadata
        assert!(!prompts.is_empty());
        for (name, description, _) in &prompts {
            assert!(!name.is_empty());
            assert!(!description.is_empty());
        }

        // Validate resources metadata - MCP spec requires 'uri' and 'name' fields
        assert!(!resources.is_empty());
        for (uri, name, _) in &resources {
            assert!(
                !uri.is_empty(),
                "Resource URI must not be empty (MCP spec requirement)"
            );
            assert!(
                !name.is_empty(),
                "Resource name must not be empty (MCP spec requirement)"
            );

            // Validate URI format - MCP spec requires valid URI format
            assert!(
                uri.contains("://"),
                "Resource URI must be valid URI format (MCP spec)"
            );
        }
    }

    /// Comprehensive integration test combining all functionality
    #[tokio::test]
    async fn test_comprehensive_integration() {
        let server = IntegrationTestServer::new();

        // Reset state
        server.reset_counter().await.unwrap();

        // Test tool functionality
        let tool_result = server
            .comprehensive_params(
                "test_string".to_string(),
                42,
                Some(true),
                Some(std::f64::consts::PI),
                vec!["item1".to_string(), "item2".to_string()],
                json!({"nested": "value"}),
            )
            .await;
        assert!(tool_result.is_ok());

        // Test prompt functionality
        let prompt_result = server
            .dynamic_content("integration_test".to_string(), None)
            .await;
        assert!(prompt_result.is_ok());

        // Test resource functionality
        let resource_result = server
            .dynamic_docs("docs://content/integration".to_string())
            .await;
        assert!(resource_result.is_ok());

        // Test concurrent operations
        let mut handles = Vec::new();
        for _ in 0..5 {
            let server_clone = server.clone();
            handles.push(task::spawn(async move {
                server_clone.increment_counter().await
            }));
        }

        for handle in handles {
            assert!(handle.await.unwrap().is_ok());
        }

        // Validate final state
        let final_count = server.counter.load(Ordering::SeqCst);
        assert_eq!(final_count, 5);
    }
}
