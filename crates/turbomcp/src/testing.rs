//! In-memory test client for ergonomic MCP server testing.
//!
//! This module provides `McpTestClient` which enables direct handler testing
//! without any network transport, making tests fast and reliable.
//!
//! # Example
//!
//! ```rust,ignore
//! use turbomcp::prelude::*;
//! use turbomcp::testing::McpTestClient;
//!
//! #[derive(Clone)]
//! struct Calculator;
//!
//! #[server(name = "calculator", version = "1.0.0")]
//! impl Calculator {
//!     #[tool]
//!     async fn add(&self, a: i64, b: i64) -> i64 {
//!         a + b
//!     }
//! }
//!
//! #[tokio::test]
//! async fn test_add() {
//!     let client = McpTestClient::new(Calculator);
//!
//!     let result = client.call_tool("add", serde_json::json!({"a": 2, "b": 3})).await.unwrap();
//!     assert_eq!(result.first_text(), Some("5"));
//! }
//! ```

use serde_json::Value;
use std::sync::Arc;

use turbomcp_core::context::RequestContext;
use turbomcp_core::error::McpResult;
use turbomcp_core::handler::McpHandler;
use turbomcp_types::{
    Prompt, PromptResult, Resource, ResourceResult, ServerInfo, Tool, ToolResult,
};

/// In-memory test client for MCP handlers.
///
/// This client enables direct testing of `McpHandler` implementations without
/// any network transport. It provides:
///
/// - Direct method invocation (no serialization overhead in tests)
/// - Rich assertion helpers
/// - Session simulation for stateful testing
/// - Minimal dependencies (no tokio runtime setup required for sync assertions)
///
/// # Example
///
/// ```rust
/// use turbomcp::testing::McpTestClient;
/// use turbomcp_core::handler::McpHandler;
/// use turbomcp_core::context::RequestContext;
/// use turbomcp_core::error::{McpError, McpResult};
/// use turbomcp_types::{Prompt, PromptResult, Resource, ResourceResult, ServerInfo, Tool, ToolResult};
/// use serde_json::Value;
/// use core::future::Future;
///
/// #[derive(Clone)]
/// struct TestServer;
///
/// impl McpHandler for TestServer {
///     fn server_info(&self) -> ServerInfo {
///         ServerInfo::new("test", "1.0.0")
///     }
///     fn list_tools(&self) -> Vec<Tool> {
///         vec![Tool::new("greet", "Say hello")]
///     }
///     fn list_resources(&self) -> Vec<Resource> { vec![] }
///     fn list_prompts(&self) -> Vec<Prompt> { vec![] }
///     fn call_tool<'a>(&'a self, name: &'a str, args: Value, _ctx: &'a RequestContext)
///         -> impl Future<Output = McpResult<ToolResult>> + Send + 'a {
///         async move {
///             match name {
///                 "greet" => {
///                     let who = args.get("name").and_then(|v| v.as_str()).unwrap_or("World");
///                     Ok(ToolResult::text(format!("Hello, {}!", who)))
///                 }
///                 _ => Err(McpError::tool_not_found(name))
///             }
///         }
///     }
///     fn read_resource<'a>(&'a self, uri: &'a str, _ctx: &'a RequestContext)
///         -> impl Future<Output = McpResult<ResourceResult>> + Send + 'a {
///         async move { Err(McpError::resource_not_found(uri)) }
///     }
///     fn get_prompt<'a>(&'a self, name: &'a str, _args: Option<Value>, _ctx: &'a RequestContext)
///         -> impl Future<Output = McpResult<PromptResult>> + Send + 'a {
///         async move { Err(McpError::prompt_not_found(name)) }
///     }
/// }
///
/// let client = McpTestClient::new(TestServer);
/// assert_eq!(client.server_info().name, "test");
/// assert_eq!(client.list_tools().len(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct McpTestClient<H: McpHandler> {
    handler: Arc<H>,
    session_id: Option<String>,
}

impl<H: McpHandler> McpTestClient<H> {
    /// Create a new test client wrapping the given handler.
    pub fn new(handler: H) -> Self {
        Self {
            handler: Arc::new(handler),
            session_id: None,
        }
    }

    /// Set a session ID for stateful testing.
    ///
    /// This allows testing session-scoped state management.
    #[must_use]
    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Get a reference to the inner handler.
    pub fn handler(&self) -> &H {
        &self.handler
    }

    // ===== Synchronous Methods =====

    /// Get server information.
    pub fn server_info(&self) -> ServerInfo {
        self.handler.server_info()
    }

    /// List all available tools.
    pub fn list_tools(&self) -> Vec<Tool> {
        self.handler.list_tools()
    }

    /// List all available resources.
    pub fn list_resources(&self) -> Vec<Resource> {
        self.handler.list_resources()
    }

    /// List all available prompts.
    pub fn list_prompts(&self) -> Vec<Prompt> {
        self.handler.list_prompts()
    }

    // ===== Async Methods =====

    /// Call a tool by name with the given arguments.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let result = client.call_tool("add", json!({"a": 1, "b": 2})).await?;
    /// assert_eq!(result.first_text(), Some("3"));
    /// ```
    pub async fn call_tool(&self, name: &str, args: Value) -> McpResult<ToolResult> {
        let ctx = self.create_context();
        self.handler.call_tool(name, args, &ctx).await
    }

    /// Call a tool with an empty arguments object.
    pub async fn call_tool_empty(&self, name: &str) -> McpResult<ToolResult> {
        self.call_tool(name, Value::Object(Default::default()))
            .await
    }

    /// Read a resource by URI.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let result = client.read_resource("file:///example.txt").await?;
    /// assert!(result.contents.first().is_some());
    /// ```
    pub async fn read_resource(&self, uri: &str) -> McpResult<ResourceResult> {
        let ctx = self.create_context();
        self.handler.read_resource(uri, &ctx).await
    }

    /// Get a prompt by name with optional arguments.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let result = client.get_prompt("greeting", Some(json!({"name": "Alice"}))).await?;
    /// assert!(!result.messages.is_empty());
    /// ```
    pub async fn get_prompt(&self, name: &str, args: Option<Value>) -> McpResult<PromptResult> {
        let ctx = self.create_context();
        self.handler.get_prompt(name, args, &ctx).await
    }

    /// Get a prompt with no arguments.
    pub async fn get_prompt_empty(&self, name: &str) -> McpResult<PromptResult> {
        self.get_prompt(name, None).await
    }

    // ===== Assertion Helpers =====

    /// Assert that a tool exists with the given name.
    ///
    /// Returns the tool definition for further assertions.
    ///
    /// # Panics
    ///
    /// Panics if no tool with the given name exists.
    pub fn assert_tool_exists(&self, name: &str) -> Tool {
        self.list_tools()
            .into_iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| {
                panic!(
                    "Tool '{}' not found. Available tools: {:?}",
                    name,
                    self.list_tools()
                        .iter()
                        .map(|t| &t.name)
                        .collect::<Vec<_>>()
                )
            })
    }

    /// Assert that a resource exists with the given URI.
    ///
    /// Returns the resource definition for further assertions.
    ///
    /// # Panics
    ///
    /// Panics if no resource with the given URI exists.
    pub fn assert_resource_exists(&self, uri: &str) -> Resource {
        self.list_resources()
            .into_iter()
            .find(|r| r.uri == uri)
            .unwrap_or_else(|| {
                panic!(
                    "Resource '{}' not found. Available resources: {:?}",
                    uri,
                    self.list_resources()
                        .iter()
                        .map(|r| &r.uri)
                        .collect::<Vec<_>>()
                )
            })
    }

    /// Assert that a prompt exists with the given name.
    ///
    /// Returns the prompt definition for further assertions.
    ///
    /// # Panics
    ///
    /// Panics if no prompt with the given name exists.
    pub fn assert_prompt_exists(&self, name: &str) -> Prompt {
        self.list_prompts()
            .into_iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| {
                panic!(
                    "Prompt '{}' not found. Available prompts: {:?}",
                    name,
                    self.list_prompts()
                        .iter()
                        .map(|p| &p.name)
                        .collect::<Vec<_>>()
                )
            })
    }

    /// Assert that no tool with the given name exists.
    ///
    /// # Panics
    ///
    /// Panics if a tool with the given name exists.
    pub fn assert_tool_not_exists(&self, name: &str) {
        if self.list_tools().iter().any(|t| t.name == name) {
            panic!("Expected tool '{}' to not exist, but it does", name);
        }
    }

    /// Assert the exact number of tools.
    ///
    /// # Panics
    ///
    /// Panics if the tool count doesn't match.
    pub fn assert_tool_count(&self, expected: usize) {
        let actual = self.list_tools().len();
        assert_eq!(
            actual, expected,
            "Expected {} tools, found {}",
            expected, actual
        );
    }

    /// Assert the exact number of resources.
    ///
    /// # Panics
    ///
    /// Panics if the resource count doesn't match.
    pub fn assert_resource_count(&self, expected: usize) {
        let actual = self.list_resources().len();
        assert_eq!(
            actual, expected,
            "Expected {} resources, found {}",
            expected, actual
        );
    }

    /// Assert the exact number of prompts.
    ///
    /// # Panics
    ///
    /// Panics if the prompt count doesn't match.
    pub fn assert_prompt_count(&self, expected: usize) {
        let actual = self.list_prompts().len();
        assert_eq!(
            actual, expected,
            "Expected {} prompts, found {}",
            expected, actual
        );
    }

    // ===== Internal Helpers =====

    fn create_context(&self) -> RequestContext {
        let mut ctx = RequestContext::default();
        if let Some(ref session_id) = self.session_id {
            ctx = ctx.with_metadata("session_id", session_id.as_str());
        }
        ctx
    }
}

/// Extension trait for ToolResult assertions.
pub trait ToolResultAssertions {
    /// Assert the result contains text matching the expected value.
    fn assert_text(&self, expected: &str);

    /// Assert the result contains text containing the substring.
    fn assert_text_contains(&self, substring: &str);

    /// Assert the result is an error.
    fn assert_is_error(&self);

    /// Assert the result is not an error.
    fn assert_is_success(&self);
}

impl ToolResultAssertions for ToolResult {
    fn assert_text(&self, expected: &str) {
        let actual = self.first_text();
        assert_eq!(
            actual,
            Some(expected),
            "Expected tool result text '{}', got {:?}",
            expected,
            actual
        );
    }

    fn assert_text_contains(&self, substring: &str) {
        let text = self.first_text().unwrap_or_else(|| {
            panic!("Expected tool result to contain text, but no text content found")
        });
        assert!(
            text.contains(substring),
            "Expected tool result to contain '{}', but got '{}'",
            substring,
            text
        );
    }

    fn assert_is_error(&self) {
        assert!(
            self.is_error.unwrap_or(false),
            "Expected tool result to be an error"
        );
    }

    fn assert_is_success(&self) {
        assert!(
            !self.is_error.unwrap_or(false),
            "Expected tool result to be successful, but got error"
        );
    }
}

/// Extension trait for McpResult<ToolResult> assertions.
pub trait McpToolResultAssertions {
    /// Unwrap and assert the result contains the expected text.
    fn assert_ok_text(self, expected: &str);

    /// Unwrap and assert the result contains text with the substring.
    fn assert_ok_contains(self, substring: &str);

    /// Assert the result is an Err.
    fn assert_err(self);

    /// Assert the result is an Err with a specific error kind.
    fn assert_err_contains(self, substring: &str);
}

impl McpToolResultAssertions for McpResult<ToolResult> {
    fn assert_ok_text(self, expected: &str) {
        match self {
            Ok(result) => result.assert_text(expected),
            Err(e) => panic!("Expected Ok with text '{}', got Err: {}", expected, e),
        }
    }

    fn assert_ok_contains(self, substring: &str) {
        match self {
            Ok(result) => result.assert_text_contains(substring),
            Err(e) => panic!("Expected Ok containing '{}', got Err: {}", substring, e),
        }
    }

    fn assert_err(self) {
        assert!(self.is_err(), "Expected Err, got Ok");
    }

    fn assert_err_contains(self, substring: &str) {
        match self {
            Ok(_) => panic!("Expected Err containing '{}', got Ok", substring),
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains(substring),
                    "Expected error containing '{}', got '{}'",
                    substring,
                    msg
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::future::Future;
    use turbomcp_core::error::McpError;
    use turbomcp_core::marker::MaybeSend;

    #[derive(Clone)]
    struct TestHandler;

    impl McpHandler for TestHandler {
        fn server_info(&self) -> ServerInfo {
            ServerInfo::new("test-server", "1.0.0")
        }

        fn list_tools(&self) -> Vec<Tool> {
            vec![
                Tool::new("greet", "Say hello"),
                Tool::new("add", "Add two numbers"),
            ]
        }

        fn list_resources(&self) -> Vec<Resource> {
            vec![Resource::new("file:///test.txt", "Test file")]
        }

        fn list_prompts(&self) -> Vec<Prompt> {
            vec![Prompt::new("greeting", "A greeting prompt")]
        }

        fn call_tool<'a>(
            &'a self,
            name: &'a str,
            args: Value,
            _ctx: &'a RequestContext,
        ) -> impl Future<Output = McpResult<ToolResult>> + MaybeSend + 'a {
            let name = name.to_string();
            async move {
                match name.as_str() {
                    "greet" => {
                        let who = args.get("name").and_then(|v| v.as_str()).unwrap_or("World");
                        Ok(ToolResult::text(format!("Hello, {}!", who)))
                    }
                    "add" => {
                        let a = args.get("a").and_then(|v| v.as_i64()).unwrap_or(0);
                        let b = args.get("b").and_then(|v| v.as_i64()).unwrap_or(0);
                        Ok(ToolResult::text((a + b).to_string()))
                    }
                    _ => Err(McpError::tool_not_found(&name)),
                }
            }
        }

        fn read_resource<'a>(
            &'a self,
            uri: &'a str,
            _ctx: &'a RequestContext,
        ) -> impl Future<Output = McpResult<ResourceResult>> + MaybeSend + 'a {
            let uri = uri.to_string();
            async move {
                if uri == "file:///test.txt" {
                    Ok(ResourceResult::text(&uri, "Test content"))
                } else {
                    Err(McpError::resource_not_found(&uri))
                }
            }
        }

        fn get_prompt<'a>(
            &'a self,
            name: &'a str,
            args: Option<Value>,
            _ctx: &'a RequestContext,
        ) -> impl Future<Output = McpResult<PromptResult>> + MaybeSend + 'a {
            let name = name.to_string();
            async move {
                if name == "greeting" {
                    let who = args
                        .and_then(|a| a.get("name").and_then(|v| v.as_str()).map(String::from))
                        .unwrap_or_else(|| "World".to_string());
                    Ok(PromptResult::user(format!("Say hello to {}", who)))
                } else {
                    Err(McpError::prompt_not_found(&name))
                }
            }
        }
    }

    #[test]
    fn test_server_info() {
        let client = McpTestClient::new(TestHandler);
        let info = client.server_info();
        assert_eq!(info.name, "test-server");
        assert_eq!(info.version, "1.0.0");
    }

    #[test]
    fn test_list_tools() {
        let client = McpTestClient::new(TestHandler);
        let tools = client.list_tools();
        assert_eq!(tools.len(), 2);
    }

    #[test]
    fn test_assert_tool_exists() {
        let client = McpTestClient::new(TestHandler);
        let tool = client.assert_tool_exists("greet");
        assert_eq!(tool.name, "greet");
    }

    #[test]
    #[should_panic(expected = "Tool 'nonexistent' not found")]
    fn test_assert_tool_exists_fails() {
        let client = McpTestClient::new(TestHandler);
        client.assert_tool_exists("nonexistent");
    }

    #[test]
    fn test_assert_tool_not_exists() {
        let client = McpTestClient::new(TestHandler);
        client.assert_tool_not_exists("nonexistent");
    }

    #[test]
    fn test_assert_tool_count() {
        let client = McpTestClient::new(TestHandler);
        client.assert_tool_count(2);
    }

    #[tokio::test]
    async fn test_call_tool() {
        let client = McpTestClient::new(TestHandler);
        let result = client
            .call_tool("greet", serde_json::json!({"name": "Alice"}))
            .await
            .unwrap();
        assert_eq!(result.first_text(), Some("Hello, Alice!"));
    }

    #[tokio::test]
    async fn test_call_tool_add() {
        let client = McpTestClient::new(TestHandler);
        let result = client
            .call_tool("add", serde_json::json!({"a": 2, "b": 3}))
            .await
            .unwrap();
        result.assert_text("5");
    }

    #[tokio::test]
    async fn test_call_tool_not_found() {
        let client = McpTestClient::new(TestHandler);
        let result = client.call_tool_empty("nonexistent").await;
        result.assert_err_contains("not found");
    }

    #[tokio::test]
    async fn test_read_resource() {
        let client = McpTestClient::new(TestHandler);
        let result = client.read_resource("file:///test.txt").await.unwrap();
        assert!(!result.contents.is_empty());
    }

    #[tokio::test]
    async fn test_get_prompt() {
        let client = McpTestClient::new(TestHandler);
        let result = client
            .get_prompt("greeting", Some(serde_json::json!({"name": "Bob"})))
            .await
            .unwrap();
        assert!(!result.messages.is_empty());
    }

    #[tokio::test]
    async fn test_with_session() {
        let client = McpTestClient::new(TestHandler).with_session("test-session-123");
        // Session ID is set (would be used for state management)
        let result = client.call_tool_empty("greet").await.unwrap();
        result.assert_text_contains("Hello");
    }

    #[test]
    fn test_tool_result_assertions() {
        let result = ToolResult::text("Hello, World!");
        result.assert_text("Hello, World!");
        result.assert_text_contains("World");
        result.assert_is_success();
    }
}
