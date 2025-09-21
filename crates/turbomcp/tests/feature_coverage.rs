//! Test coverage for optional features: schema-generation and uri-templates

use turbomcp::prelude::*;

#[cfg(feature = "uri-templates")]
use turbomcp::uri::UriTemplate;

// Remove unused import - we now test actual macro-generated schemas

#[cfg(feature = "uri-templates")]
#[tokio::test]
async fn test_uri_templates_comprehensive() {
    // Test simple URI template
    let simple_template = UriTemplate::new("config://settings/{section}").unwrap();

    // Test matching URIs
    assert!(
        simple_template
            .matches("config://settings/database")
            .is_some()
    );
    assert!(simple_template.matches("config://settings/auth").is_some());
    assert!(
        simple_template
            .matches("config://settings/logging")
            .is_some()
    );

    // Test non-matching URIs
    assert!(simple_template.matches("config://other/database").is_none());
    assert!(
        simple_template
            .matches("file://settings/database")
            .is_none()
    );
    assert!(simple_template.matches("config://settings").is_none());
    assert!(simple_template.matches("config://settings/").is_none());

    // Test parameter extraction
    let params = simple_template
        .matches("config://settings/database")
        .unwrap();
    assert_eq!(params.get("section"), Some(&"database".to_string()));

    let params2 = simple_template.matches("config://settings/auth").unwrap();
    assert_eq!(params2.get("section"), Some(&"auth".to_string()));

    // Test complex URI template with multiple parameters
    let complex_template = UriTemplate::new("api://v{version}/{service}/users/{user_id}").unwrap();

    assert!(
        complex_template
            .matches("api://v1/auth/users/123")
            .is_some()
    );
    assert!(
        complex_template
            .matches("api://v2/billing/users/456")
            .is_some()
    );
    assert!(
        complex_template
            .matches("api://v1/auth/posts/123")
            .is_none()
    );
    assert!(complex_template.matches("api://v1/users/123").is_none());

    let complex_params = complex_template.matches("api://v1/auth/users/123").unwrap();
    assert_eq!(complex_params.get("version"), Some(&"1".to_string()));
    assert_eq!(complex_params.get("service"), Some(&"auth".to_string()));
    assert_eq!(complex_params.get("user_id"), Some(&"123".to_string()));

    // Test URI template with no parameters
    let no_params_template = UriTemplate::new("static://resources/data.json").unwrap();
    assert!(
        no_params_template
            .matches("static://resources/data.json")
            .is_some()
    );
    assert!(
        no_params_template
            .matches("static://resources/other.json")
            .is_none()
    );

    let no_params = no_params_template
        .matches("static://resources/data.json")
        .unwrap();
    assert!(no_params.is_empty());

    // Test edge cases
    let edge_template = UriTemplate::new("file:///{path}").unwrap();
    assert!(
        edge_template
            .matches("file:///home/user/file.txt")
            .is_some()
    );
    assert!(edge_template.matches("file:///var/log/app.log").is_some());

    let edge_params = edge_template.matches("file:///home/user/file.txt").unwrap();
    assert_eq!(
        edge_params.get("path"),
        Some(&"home/user/file.txt".to_string())
    );

    // Test parameter extraction function directly using UriTemplate
    let direct_template = UriTemplate::new("config://settings/{section}").unwrap();
    let direct_params = direct_template.matches("config://settings/database");
    assert!(direct_params.is_some());
    assert_eq!(
        direct_params.unwrap().get("section"),
        Some(&"database".to_string())
    );

    // Test with non-matching pattern
    let no_match = direct_template.matches("other://settings/database");
    assert!(no_match.is_none());

    // Test with special characters in parameters
    let special_template = UriTemplate::new("data://{dataset}/query").unwrap();
    assert!(
        special_template
            .matches("data://my-dataset_123/query")
            .is_some()
    );

    let special_params = special_template
        .matches("data://my-dataset_123/query")
        .unwrap();
    assert_eq!(
        special_params.get("dataset"),
        Some(&"my-dataset_123".to_string())
    );
}

#[cfg(feature = "uri-templates")]
#[tokio::test]
async fn test_uri_template_error_cases() {
    // Test invalid regex patterns (these should fail with proper error)
    let result = UriTemplate::new("invalid[regex");
    // URI template creation should fail for invalid regex patterns
    assert!(result.is_err());

    // Test empty template
    let empty = UriTemplate::new("");
    assert!(empty.is_ok());
    let empty_template = empty.unwrap();
    assert!(empty_template.matches("anything").is_none());
    assert!(empty_template.matches("").is_some());

    // Test template with no parameters but complex pattern
    let complex_no_params = UriTemplate::new("https://api.example.com/v1/users").unwrap();
    assert!(
        complex_no_params
            .matches("https://api.example.com/v1/users")
            .is_some()
    );
    assert!(
        complex_no_params
            .matches("https://api.example.com/v2/users")
            .is_none()
    );

    // Test parameter extraction from non-matching URI
    let template = UriTemplate::new("config://settings/{section}").unwrap();
    let no_match = template.matches("different://pattern/test");
    assert!(no_match.is_none());
}

#[cfg(feature = "schema-generation")]
#[tokio::test]
async fn test_schema_generation_comprehensive() {
    use turbomcp::prelude::*;

    // Test actual macro-generated schemas instead of schemars
    #[derive(Clone)]
    struct TestSchemaServer;

    #[server]
    #[allow(dead_code)]
    impl TestSchemaServer {
        #[tool("Test tool with basic types")]
        async fn basic_types(&self, name: String, age: u32, active: bool) -> McpResult<String> {
            Ok(format!("User: {}, age: {}, active: {}", name, age, active))
        }

        #[tool("Test tool with complex types")]
        async fn complex_types(
            &self,
            user_id: u64,
            name: String,
            #[allow(unused_variables)] email: Option<String>,
            tags: Vec<String>,
        ) -> McpResult<String> {
            Ok(format!(
                "User {} ({}) with {} tags",
                name,
                user_id,
                tags.len()
            ))
        }

        #[tool("Test tool with optional parameters")]
        async fn optional_params(
            &self,
            required_param: String,
            optional_param: Option<String>,
        ) -> McpResult<String> {
            match optional_param {
                Some(val) => Ok(format!("{}: {}", required_param, val)),
                None => Ok(required_param),
            }
        }
    }

    // Test actual macro-generated schema for basic types tool
    let (name, desc, schema) = TestSchemaServer::basic_types_metadata();
    assert_eq!(name, "basic_types");
    assert!(!desc.is_empty());
    assert!(!schema.is_null());

    // Validate schema structure from actual macro generation
    let properties = schema["properties"].as_object().unwrap();
    assert!(properties.contains_key("name"));
    assert!(properties.contains_key("age"));
    assert!(properties.contains_key("active"));

    // Verify type inference from Rust to JSON Schema
    assert_eq!(properties["name"]["type"], "string");
    assert_eq!(properties["age"]["type"], "integer");
    assert_eq!(properties["active"]["type"], "boolean");

    // Test complex types schema
    let (_, _, complex_schema) = TestSchemaServer::complex_types_metadata();
    let complex_props = complex_schema["properties"].as_object().unwrap();

    // Verify nested type handling
    assert!(complex_props.contains_key("user_id"));
    assert!(complex_props.contains_key("tags"));
    assert!(complex_props.contains_key("email"));
    assert_eq!(complex_props["user_id"]["type"], "integer");
    assert_eq!(complex_props["tags"]["type"], "array");

    // Test optional parameter handling
    let (_, _, optional_schema) = TestSchemaServer::optional_params_metadata();
    let optional_props = optional_schema["properties"].as_object().unwrap();

    // Required parameters should be in required array
    let required = optional_schema["required"].as_array().unwrap();
    assert!(required.contains(&serde_json::Value::String("required_param".to_string())));
    assert!(!required.contains(&serde_json::Value::String("optional_param".to_string())));

    // Both should be in properties
    assert!(optional_props.contains_key("required_param"));
    assert!(optional_props.contains_key("optional_param"));
}

// Test that features work correctly when disabled
#[cfg(not(feature = "uri-templates"))]
#[tokio::test]
async fn test_uri_templates_disabled() {
    // When uri-templates feature is disabled, the module shouldn't be available
    // This test just ensures the feature flag works correctly

    // We can't import turbomcp::uri when the feature is disabled
    // So we just test that basic TurboMCP functionality still works
    use turbomcp::prelude::*;

    let content = text("URI templates disabled");
    match content {
        Content::Text(text_content) => {
            assert_eq!(text_content.text, "URI templates disabled");
        }
        _ => panic!("Expected TextContent"),
    }
}

#[cfg(not(feature = "schema-generation"))]
#[tokio::test]
async fn test_schema_generation_disabled() {
    // When schema-generation feature is disabled, the module shouldn't be available
    // This test just ensures the feature flag works correctly

    use turbomcp::prelude::*;

    let content = text("Schema generation disabled");
    match content {
        Content::Text(text_content) => {
            assert_eq!(text_content.text, "Schema generation disabled");
        }
        _ => panic!("Expected TextContent"),
    }
}

// Test feature combinations
#[cfg(all(feature = "uri-templates", feature = "schema-generation"))]
#[tokio::test]
async fn test_combined_features() {
    use serde::{Deserialize, Serialize};
    use turbomcp::uri::UriTemplate;

    // Test using both features together
    #[derive(Serialize, Deserialize, schemars::JsonSchema)]
    #[allow(dead_code)]
    struct ResourceRequest {
        template: String,
        parameters: std::collections::HashMap<String, String>,
    }

    // Test actual macro-generated schema instead of schemars
    #[derive(Clone)]
    struct ResourceServer;

    #[server]
    #[allow(dead_code)]
    impl ResourceServer {
        #[tool("Process resource request")]
        async fn process_request(
            &self,
            template: String,
            parameters: std::collections::HashMap<String, String>,
        ) -> McpResult<String> {
            Ok(format!(
                "Processing {} with {} params",
                template,
                parameters.len()
            ))
        }
    }

    let (_, _, schema) = ResourceServer::process_request_metadata();

    // Use URI template functionality
    let template = UriTemplate::new("resource://{type}/{id}").unwrap();
    assert!(template.matches("resource://user/123").is_some());

    let params = template.matches("resource://user/123").unwrap();
    assert_eq!(params.get("type"), Some(&"user".to_string()));
    assert_eq!(params.get("id"), Some(&"123".to_string()));

    // Both features should work together seamlessly
    let schema_str = schema.to_string();
    assert!(schema_str.contains("template"));
    assert!(schema_str.contains("parameters"));
}

#[cfg(feature = "uri-templates")]
#[tokio::test]
async fn test_uri_template_performance() {
    use std::time::Instant;

    let template = UriTemplate::new("api://v{version}/{service}/users/{user_id}").unwrap();
    let test_uri = "api://v1/auth/users/12345";

    let start = Instant::now();

    // Test matching performance
    for _ in 0..1000 {
        assert!(template.matches(test_uri).is_some());
    }

    let match_duration = start.elapsed();
    assert!(
        match_duration.as_millis() < 100,
        "URI matching should be fast"
    );

    let start = Instant::now();

    // Test parameter extraction performance
    for _ in 0..1000 {
        let params = template.matches(test_uri).unwrap();
        assert_eq!(params.len(), 3);
    }

    let extract_duration = start.elapsed();
    assert!(
        extract_duration.as_millis() < 200,
        "Parameter extraction should be fast"
    );
}

#[cfg(feature = "schema-generation")]
#[tokio::test]
async fn test_schema_generation_performance() {
    use serde::{Deserialize, Serialize};
    use std::time::Instant;

    #[derive(Serialize, Deserialize, schemars::JsonSchema)]
    #[allow(dead_code)]
    struct TestStruct {
        field1: String,
        field2: i32,
        field3: Vec<String>,
        field4: Option<bool>,
    }

    let start = Instant::now();

    // Test schema generation performance using actual macros
    #[derive(Clone)]
    struct PerformanceServer;

    #[server]
    #[allow(dead_code)]
    impl PerformanceServer {
        #[tool("Performance test tool")]
        async fn test_performance(
            &self,
            #[allow(unused_variables)] field1: String,
            #[allow(unused_variables)] field2: i32,
            #[allow(unused_variables)] field3: Vec<String>,
            #[allow(unused_variables)] field4: Option<bool>,
        ) -> McpResult<String> {
            Ok(format!("Processed {} fields", 4))
        }
    }

    // Generate schema multiple times using actual macro implementation
    for _ in 0..100 {
        let (_, _, _schema) = PerformanceServer::test_performance_metadata();
        // Validate the schema is properly generated
        assert!(!_schema.is_null());
    }

    let duration = start.elapsed();
    assert!(
        duration.as_millis() < 1000,
        "Schema generation should be reasonably fast"
    );
}

// Test real-world usage scenarios
#[cfg(all(feature = "uri-templates", feature = "schema-generation"))]
#[tokio::test]
async fn test_real_world_scenarios() {
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;
    use turbomcp::uri::UriTemplate;

    // Scenario 1: File system resource with schema
    #[derive(Serialize, Deserialize, schemars::JsonSchema)]
    #[allow(dead_code)]
    struct FileResource {
        path: String,
        size: u64,
        modified: String,
        permissions: String,
    }

    let file_template = UriTemplate::new("file://{path}").unwrap();

    // Test actual macro-generated schema instead of schemars
    #[derive(Clone)]
    struct FileServer;

    #[server]
    #[allow(dead_code)]
    impl FileServer {
        #[tool("Get file info")]
        async fn get_file_info(
            &self,
            path: String,
            size: u64,
            #[allow(unused_variables)] modified: String,
            #[allow(unused_variables)] permissions: String,
        ) -> McpResult<String> {
            Ok(format!("File {} ({} bytes)", path, size))
        }
    }

    let (_, _, file_schema) = FileServer::get_file_info_metadata();

    assert!(
        file_template
            .matches("file:///home/user/document.txt")
            .is_some()
    );
    assert!(
        file_schema["properties"]
            .as_object()
            .unwrap()
            .contains_key("path")
    );

    // Scenario 2: API endpoint with parameters
    #[derive(Serialize, Deserialize, schemars::JsonSchema)]
    #[allow(dead_code)]
    struct ApiResponse {
        success: bool,
        data: serde_json::Value,
        errors: Vec<String>,
    }

    let api_template = UriTemplate::new("https://api.example.com/{version}/{endpoint}").unwrap();

    // Test actual macro-generated schema instead of schemars
    #[derive(Clone)]
    struct ApiServer;

    #[server]
    #[allow(dead_code)]
    impl ApiServer {
        #[tool("API response handler")]
        async fn handle_response(
            &self,
            success: bool,
            #[allow(unused_variables)] data: serde_json::Value,
            errors: Vec<String>,
        ) -> McpResult<String> {
            if success {
                Ok("Success".to_string())
            } else {
                Ok(format!("Failed with {} errors", errors.len()))
            }
        }
    }

    let (_, _, _api_schema) = ApiServer::handle_response_metadata();

    assert!(
        api_template
            .matches("https://api.example.com/v1/users")
            .is_some()
    );
    let params = api_template
        .matches("https://api.example.com/v2/posts")
        .unwrap();
    assert_eq!(params.get("version"), Some(&"v2".to_string()));
    assert_eq!(params.get("endpoint"), Some(&"posts".to_string()));

    // Scenario 3: Database resource
    #[derive(Serialize, Deserialize, schemars::JsonSchema)]
    #[allow(dead_code)]
    struct DatabaseRecord {
        id: u64,
        table: String,
        data: HashMap<String, serde_json::Value>,
    }

    let db_template = UriTemplate::new("db://{database}/{table}/{id}").unwrap();

    // Test actual macro-generated schema instead of schemars
    #[derive(Clone)]
    struct DatabaseServer;

    #[server]
    #[allow(dead_code)]
    impl DatabaseServer {
        #[tool("Query database record")]
        async fn query_record(
            &self,
            id: u64,
            table: String,
            data: HashMap<String, serde_json::Value>,
        ) -> McpResult<String> {
            Ok(format!(
                "Queried {} from table {} with {} fields",
                id,
                table,
                data.len()
            ))
        }
    }

    let (_, _, db_schema) = DatabaseServer::query_record_metadata();

    assert!(db_template.matches("db://myapp/users/123").is_some());
    let db_params = db_template.matches("db://myapp/orders/456").unwrap();
    assert_eq!(db_params.get("database"), Some(&"myapp".to_string()));
    assert_eq!(db_params.get("table"), Some(&"orders".to_string()));
    assert_eq!(db_params.get("id"), Some(&"456".to_string()));

    let db_props = db_schema["properties"].as_object().unwrap();
    assert!(db_props.contains_key("table"));
    assert!(db_props.contains_key("data"));
}
