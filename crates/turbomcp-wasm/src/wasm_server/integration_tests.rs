//! Integration tests for WASM server API
//!
//! Tests the full API surface to ensure ergonomics and consistency with turbomcp patterns.

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::super::*;

    // ========================================================================
    // Argument Structs
    // ========================================================================

    #[derive(serde::Deserialize, schemars::JsonSchema)]
    struct GreetArgs {
        name: String,
    }

    #[derive(serde::Deserialize, schemars::JsonSchema)]
    struct AddArgs {
        a: i64,
        b: i64,
    }

    #[derive(serde::Deserialize, schemars::JsonSchema)]
    struct DivideArgs {
        a: f64,
        b: f64,
    }

    #[derive(serde::Deserialize, schemars::JsonSchema)]
    struct SearchArgs {
        query: String,
        #[serde(default)]
        limit: Option<u32>,
    }

    #[derive(serde::Serialize)]
    struct SearchResult {
        items: Vec<String>,
        total: usize,
    }

    // ========================================================================
    // Test: Simple String Returns
    // ========================================================================

    #[test]
    fn test_tool_string_return() {
        async fn greet(args: GreetArgs) -> String {
            format!("Hello, {}!", args.name)
        }

        let server = McpServer::builder("test", "1.0.0")
            .tool("greet", "Say hello", greet)
            .build();

        assert_eq!(server.tools().len(), 1);
        let tool = &server.tools()[0];
        assert_eq!(tool.name, "greet");
        assert_eq!(tool.description.as_deref(), Some("Say hello"));
    }

    // ========================================================================
    // Test: Numeric Returns
    // ========================================================================

    #[test]
    fn test_tool_numeric_operations() {
        async fn add(args: AddArgs) -> i64 {
            args.a + args.b
        }

        async fn multiply(args: AddArgs) -> i64 {
            args.a * args.b
        }

        async fn divide(args: DivideArgs) -> f64 {
            args.a / args.b
        }

        let server = McpServer::builder("calc", "1.0.0")
            .tool("add", "Add two numbers", add)
            .tool("multiply", "Multiply two numbers", multiply)
            .tool("divide", "Divide two numbers", divide)
            .build();

        assert_eq!(server.tools().len(), 3);
    }

    #[test]
    fn test_tool_bool_return() {
        #[derive(serde::Deserialize, schemars::JsonSchema)]
        struct CheckArgs {
            value: i64,
        }

        async fn is_positive(args: CheckArgs) -> bool {
            args.value > 0
        }

        let server = McpServer::builder("check", "1.0.0")
            .tool("is_positive", "Check if positive", is_positive)
            .build();

        assert_eq!(server.tools().len(), 1);
    }

    // ========================================================================
    // Test: Error Handling with Result
    // ========================================================================

    #[test]
    fn test_tool_error_handling() {
        async fn divide(args: DivideArgs) -> Result<String, ToolError> {
            if args.b == 0.0 {
                return Err(ToolError::new("Cannot divide by zero"));
            }
            Ok(format!("{}", args.a / args.b))
        }

        let server = McpServer::builder("calc", "1.0.0")
            .tool("divide", "Divide two numbers", divide)
            .build();

        assert_eq!(server.tools().len(), 1);
    }

    // ========================================================================
    // Test: JSON Response
    // ========================================================================

    #[test]
    fn test_tool_json_response() {
        async fn search(args: SearchArgs) -> Json<SearchResult> {
            let items = vec![format!("Result for: {}", args.query)];
            Json(SearchResult {
                total: items.len(),
                items,
            })
        }

        let server = McpServer::builder("search", "1.0.0")
            .tool("search", "Search items", search)
            .build();

        assert_eq!(server.tools().len(), 1);
    }

    // ========================================================================
    // Test: No Arguments Tool
    // ========================================================================

    #[test]
    fn test_tool_no_args() {
        async fn get_time() -> String {
            "2024-01-01T00:00:00Z".to_string()
        }

        let server = McpServer::builder("time", "1.0.0")
            .tool_no_args("time", "Get current time", get_time)
            .build();

        assert_eq!(server.tools().len(), 1);
        let tool = &server.tools()[0];
        // Schema should be empty object
        assert!(
            tool.input_schema.properties.is_none()
                || tool.input_schema.properties.as_ref().unwrap().is_empty()
        );
    }

    // ========================================================================
    // Test: Raw JSON Tool
    // ========================================================================

    #[test]
    fn test_tool_raw_json() {
        async fn dynamic(args: serde_json::Value) -> String {
            format!("Received: {}", args)
        }

        let server = McpServer::builder("dynamic", "1.0.0")
            .tool_raw("dynamic", "Dynamic tool", dynamic)
            .build();

        assert_eq!(server.tools().len(), 1);
    }

    // ========================================================================
    // Test: Multiple Tools
    // ========================================================================

    #[test]
    fn test_multiple_tools() {
        async fn tool1(args: GreetArgs) -> String {
            format!("Tool 1: {}", args.name)
        }
        async fn tool2(args: AddArgs) -> String {
            format!("Tool 2: {}", args.a + args.b)
        }
        async fn tool3() -> String {
            "Tool 3".to_string()
        }

        let server = McpServer::builder("multi", "1.0.0")
            .tool("tool1", "First tool", tool1)
            .tool("tool2", "Second tool", tool2)
            .tool_no_args("tool3", "Third tool", tool3)
            .build();

        assert_eq!(server.tools().len(), 3);
    }

    // ========================================================================
    // Test: Resources
    // ========================================================================

    #[test]
    fn test_resource_registration() {
        async fn read_config(_uri: String) -> ResourceResult {
            ResourceResult::text("config://app", r#"{"theme": "dark"}"#)
        }

        let server = McpServer::builder("config", "1.0.0")
            .resource(
                "config://app",
                "Config",
                "Application configuration",
                read_config,
            )
            .build();

        assert_eq!(server.resources().len(), 1);
        let resource = &server.resources()[0];
        assert_eq!(resource.uri, "config://app");
        assert_eq!(resource.name, "Config");
    }

    // ========================================================================
    // Test: Resource Template
    // ========================================================================

    #[test]
    fn test_resource_template() {
        async fn read_user(uri: String) -> Result<ResourceResult, ToolError> {
            let id = uri
                .split('/')
                .next_back()
                .ok_or_else(|| ToolError::new("Invalid URI"))?;
            Ok(ResourceResult::text(&uri, format!("User {}", id)))
        }

        let server = McpServer::builder("users", "1.0.0")
            .resource_template("user://{id}", "User", "User profile by ID", read_user)
            .build();

        assert_eq!(server.resource_templates().len(), 1);
        let template = &server.resource_templates()[0];
        assert_eq!(template.uri_template, "user://{id}");
    }

    // ========================================================================
    // Test: Prompts
    // ========================================================================

    #[test]
    fn test_prompt_no_args() {
        async fn greeting() -> PromptResult {
            PromptResult::user("Hello! How can I help?")
        }

        let server = McpServer::builder("chat", "1.0.0")
            .prompt_no_args("greeting", "Default greeting", greeting)
            .build();

        assert_eq!(server.prompts().len(), 1);
        let prompt = &server.prompts()[0];
        assert_eq!(prompt.name, "greeting");
        assert!(prompt.arguments.is_none());
    }

    // ========================================================================
    // Test: Schema Generation
    // ========================================================================

    #[test]
    fn test_schema_generation() {
        #[derive(serde::Deserialize, schemars::JsonSchema)]
        struct ComplexArgs {
            /// The user's name
            name: String,
            /// User's age in years
            age: u32,
            /// Optional email address
            #[serde(default)]
            email: Option<String>,
        }

        async fn complex(_args: ComplexArgs) -> String {
            "ok".to_string()
        }

        let server = McpServer::builder("test", "1.0.0")
            .tool("complex", "Complex tool", complex)
            .build();

        let tool = &server.tools()[0];
        let props = tool.input_schema.properties.as_ref().unwrap();

        // Should have all properties
        assert!(props.contains_key("name"));
        assert!(props.contains_key("age"));
        assert!(props.contains_key("email"));

        // Required should only have non-optional fields
        let required = tool.input_schema.required.as_ref().unwrap();
        assert!(required.contains(&"name".to_string()));
        assert!(required.contains(&"age".to_string()));
        assert!(!required.contains(&"email".to_string()));
    }

    // ========================================================================
    // Test: Server Metadata
    // ========================================================================

    #[test]
    fn test_server_metadata() {
        let server = McpServer::builder("my-server", "2.0.0")
            .description("A test server")
            .instructions("Use this server for testing")
            .build();

        // Server info is internal, but we can verify tools/resources are empty
        assert!(server.tools().is_empty());
        assert!(server.resources().is_empty());
        assert!(server.prompts().is_empty());
    }

    // ========================================================================
    // Test: Full Server Example
    // ========================================================================

    #[test]
    fn test_full_server() {
        // This mirrors what a real Cloudflare Worker would look like

        #[derive(serde::Deserialize, schemars::JsonSchema)]
        struct EchoArgs {
            message: String,
        }

        async fn echo(args: EchoArgs) -> String {
            args.message
        }

        async fn ping() -> String {
            "pong".to_string()
        }

        async fn read_status(_uri: String) -> ResourceResult {
            ResourceResult::json(
                "status://server",
                &serde_json::json!({
                    "healthy": true,
                    "uptime": 3600
                }),
            )
            .unwrap()
        }

        async fn default_prompt() -> PromptResult {
            PromptResult::user("You are a helpful assistant.").add_assistant("I'm ready to help!")
        }

        let server = McpServer::builder("full-server", "1.0.0")
            .description("A full-featured MCP server")
            .instructions("This server provides echo, ping, and status functionality")
            // Tools
            .tool("echo", "Echo a message back", echo)
            .tool_no_args("ping", "Health check", ping)
            // Resources
            .resource(
                "status://server",
                "Server Status",
                "Current server status",
                read_status,
            )
            // Prompts
            .prompt_no_args("default", "Default system prompt", default_prompt)
            .build();

        // Verify all registrations
        assert_eq!(server.tools().len(), 2);
        assert_eq!(server.resources().len(), 1);
        assert_eq!(server.prompts().len(), 1);

        // Verify tool names
        let tool_names: Vec<_> = server.tools().iter().map(|t| t.name.as_str()).collect();
        assert!(tool_names.contains(&"echo"));
        assert!(tool_names.contains(&"ping"));
    }

    // ========================================================================
    // Test: Error Conversion
    // ========================================================================

    #[test]
    fn test_error_conversions() {
        // Test that common error types convert to ToolError
        let _: ToolError = "error message".into();
        let _: ToolError = String::from("error").into();

        // Test ToolError::new
        let err = ToolError::new("custom error");
        assert_eq!(err.to_string(), "custom error");
    }

    // ========================================================================
    // Test: IntoToolResponse implementations
    // ========================================================================

    #[test]
    fn test_into_tool_response_variants() {
        use super::super::response::IntoToolResponse;

        // String
        let result = "hello".into_tool_response();
        assert!(result.is_error.is_none());

        // ToolResult
        let result = ToolResult::text("direct").into_tool_response();
        assert!(result.is_error.is_none());

        // ToolError
        let result = ToolError::new("error").into_tool_response();
        assert_eq!(result.is_error, Some(true));

        // Json
        let result = Json(serde_json::json!({"key": "value"})).into_tool_response();
        assert!(result.is_error.is_none());

        // Result<T, E> - Ok
        let result: Result<String, ToolError> = Ok("success".into());
        let result = result.into_tool_response();
        assert!(result.is_error.is_none());

        // Result<T, E> - Err
        let result: Result<String, ToolError> = Err(ToolError::new("failed"));
        let result = result.into_tool_response();
        assert_eq!(result.is_error, Some(true));

        // ()
        let result = ().into_tool_response();
        assert!(result.content.is_empty());

        // Option - Some
        let result = Some("value").into_tool_response();
        assert!(result.is_error.is_none());

        // Option - None
        let result: Option<String> = None;
        let result = result.into_tool_response();
        assert!(result.is_error.is_none());
    }
}
