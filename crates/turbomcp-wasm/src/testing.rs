//! In-memory test client for ergonomic MCP server testing in WASM environments.
//!
//! This module provides `McpTestClient` which enables direct handler testing
//! without any network transport, making tests fast and reliable.
//!
//! # Example
//!
//! ```rust,ignore
//! use turbomcp_wasm::wasm_server::*;
//! use turbomcp_wasm::testing::McpTestClient;
//! use serde::Deserialize;
//! use schemars::JsonSchema;
//!
//! #[derive(Clone)]
//! struct Calculator;
//!
//! #[derive(Deserialize, JsonSchema)]
//! struct AddArgs {
//!     a: i64,
//!     b: i64,
//! }
//!
//! impl Calculator {
//!     fn into_mcp_server(self) -> McpServer {
//!         McpServer::builder("calculator", "1.0.0")
//!             .tool("add", "Add two numbers", |args: AddArgs| async move {
//!                 format!("{}", args.a + args.b)
//!             })
//!             .build()
//!     }
//! }
//!
//! #[wasm_bindgen_test]
//! async fn test_add() {
//!     let server = Calculator.into_mcp_server();
//!     let client = McpTestClient::new(server);
//!
//!     let result = client.call_tool("add", serde_json::json!({"a": 2, "b": 3})).await.unwrap();
//!     assert_eq!(result.first_text(), Some("5".to_string()));
//! }
//! ```

use std::sync::Arc;

use serde_json::Value;

use crate::wasm_server::{McpServer, PromptResult, RequestContext, ResourceResult, ToolResult};
use turbomcp_core::types::prompts::Prompt;
use turbomcp_core::types::resources::Resource;
use turbomcp_core::types::tools::Tool;

/// In-memory test client for WASM MCP servers.
///
/// This client enables direct testing of `McpServer` implementations without
/// any network transport. It provides:
///
/// - Direct method invocation (no serialization overhead in tests)
/// - Rich assertion helpers
/// - Session simulation for stateful testing
/// - Compatible with `wasm-bindgen-test`
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_wasm::testing::McpTestClient;
/// use turbomcp_wasm::wasm_server::McpServer;
///
/// let server = McpServer::builder("test", "1.0.0")
///     .tool("greet", "Say hello", |args: GreetArgs| async move {
///         format!("Hello, {}!", args.name)
///     })
///     .build();
///
/// let client = McpTestClient::new(server);
/// assert_eq!(client.server_info().name, "test");
/// ```
#[derive(Clone)]
pub struct McpTestClient {
    server: McpServer,
    session_id: Option<String>,
    user_id: Option<String>,
    roles: Vec<String>,
    metadata: std::collections::HashMap<String, String>,
}

impl McpTestClient {
    /// Create a new test client wrapping the given server.
    pub fn new(server: McpServer) -> Self {
        Self {
            server,
            session_id: None,
            user_id: None,
            roles: Vec::new(),
            metadata: std::collections::HashMap::new(),
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

    /// Set a user ID for authentication testing.
    #[must_use]
    pub fn with_user(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    /// Add a role for authorization testing.
    ///
    /// Roles are stored in the `auth.roles` metadata field.
    #[must_use]
    pub fn with_role(mut self, role: impl Into<String>) -> Self {
        self.roles.push(role.into());
        self
    }

    /// Set roles for authorization testing.
    ///
    /// Roles are stored in the `auth.roles` metadata field.
    #[must_use]
    pub fn with_roles(mut self, roles: Vec<String>) -> Self {
        self.roles = roles;
        self
    }

    /// Add custom metadata for testing.
    #[must_use]
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Get a reference to the inner server.
    pub fn server(&self) -> &McpServer {
        &self.server
    }

    // ===== Synchronous Methods =====

    /// Get server information.
    pub fn server_info(&self) -> (&str, &str) {
        (
            &self.server.server_info.name,
            &self.server.server_info.version,
        )
    }

    /// Get server name.
    pub fn server_name(&self) -> &str {
        &self.server.server_info.name
    }

    /// Get server version.
    pub fn server_version(&self) -> &str {
        &self.server.server_info.version
    }

    /// List all available tools.
    pub fn list_tools(&self) -> Vec<&Tool> {
        self.server.tools()
    }

    /// List all available resources.
    pub fn list_resources(&self) -> Vec<&Resource> {
        self.server.resources()
    }

    /// List all available prompts.
    pub fn list_prompts(&self) -> Vec<&Prompt> {
        self.server.prompts()
    }

    // ===== Async Methods =====

    /// Call a tool by name with the given arguments.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let result = client.call_tool("add", json!({"a": 1, "b": 2})).await?;
    /// assert_eq!(result.first_text(), Some("3".to_string()));
    /// ```
    pub async fn call_tool(&self, name: &str, args: Value) -> Result<ToolResult, String> {
        let ctx = Arc::new(self.create_context());
        self.server.call_tool_internal(name, args, ctx).await
    }

    /// Call a tool with an empty arguments object.
    pub async fn call_tool_empty(&self, name: &str) -> Result<ToolResult, String> {
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
    pub async fn read_resource(&self, uri: &str) -> Result<ResourceResult, String> {
        let ctx = Arc::new(self.create_context());
        self.server.read_resource_internal(uri, ctx).await
    }

    /// Get a prompt by name with optional arguments.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let result = client.get_prompt("greeting", Some(json!({"name": "Alice"}))).await?;
    /// assert!(!result.messages.is_empty());
    /// ```
    pub async fn get_prompt(
        &self,
        name: &str,
        args: Option<Value>,
    ) -> Result<PromptResult, String> {
        let ctx = Arc::new(self.create_context());
        self.server.get_prompt_internal(name, args, ctx).await
    }

    /// Get a prompt with no arguments.
    pub async fn get_prompt_empty(&self, name: &str) -> Result<PromptResult, String> {
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
    pub fn assert_tool_exists(&self, name: &str) -> &Tool {
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
    pub fn assert_resource_exists(&self, uri: &str) -> &Resource {
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
    pub fn assert_prompt_exists(&self, name: &str) -> &Prompt {
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
        let mut ctx = RequestContext::new();

        if let Some(ref session_id) = self.session_id {
            ctx = ctx.with_session_id(session_id.clone());
        }

        if let Some(ref user_id) = self.user_id {
            ctx = ctx.with_user_id(user_id.clone());
        }

        // Set roles in auth metadata
        if !self.roles.is_empty() {
            ctx = ctx.with_metadata(
                "auth",
                serde_json::json!({
                    "roles": self.roles
                }),
            );
        }

        for (key, value) in &self.metadata {
            ctx = ctx.with_metadata(key.clone(), value.clone());
        }

        ctx
    }
}

/// Extension trait for ToolResult assertions.
pub trait ToolResultAssertions {
    /// Get the first text content, if any.
    fn first_text(&self) -> Option<String>;

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
    fn first_text(&self) -> Option<String> {
        self.content
            .first()
            .and_then(|c| c.as_text().map(|s| s.to_string()))
    }

    fn assert_text(&self, expected: &str) {
        let actual = self.first_text();
        assert_eq!(
            actual.as_deref(),
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

/// Extension trait for Result<ToolResult, String> assertions.
pub trait ToolResultExt {
    /// Unwrap and assert the result contains the expected text.
    fn assert_ok_text(self, expected: &str);

    /// Unwrap and assert the result contains text with the substring.
    fn assert_ok_contains(self, substring: &str);

    /// Assert the result is an Err.
    fn assert_err(self);

    /// Assert the result is an Err with a specific error containing the substring.
    fn assert_err_contains(self, substring: &str);
}

impl ToolResultExt for Result<ToolResult, String> {
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
                assert!(
                    e.contains(substring),
                    "Expected error containing '{}', got '{}'",
                    substring,
                    e
                );
            }
        }
    }
}

/// Extension trait for ResourceResult assertions.
pub trait ResourceResultAssertions {
    /// Get the first text content, if any.
    fn first_text(&self) -> Option<String>;

    /// Assert the result has the expected URI.
    fn assert_uri(&self, expected: &str);

    /// Assert the result contains text matching the expected value.
    fn assert_text(&self, expected: &str);

    /// Assert the result contains text containing the substring.
    fn assert_text_contains(&self, substring: &str);
}

impl ResourceResultAssertions for ResourceResult {
    fn first_text(&self) -> Option<String> {
        self.contents.first().and_then(|c| c.text.clone())
    }

    fn assert_uri(&self, expected: &str) {
        let actual = self.contents.first().map(|c| c.uri.as_str());
        assert_eq!(
            actual,
            Some(expected),
            "Expected resource URI '{}', got {:?}",
            expected,
            actual
        );
    }

    fn assert_text(&self, expected: &str) {
        let actual = self.first_text();
        // Note: as_deref() converts Option<String> to Option<&str> for comparison
        #[allow(clippy::needless_option_as_deref)]
        let actual_str = actual.as_deref();
        assert_eq!(
            actual_str,
            Some(expected),
            "Expected resource text '{}', got {:?}",
            expected,
            actual
        );
    }

    fn assert_text_contains(&self, substring: &str) {
        let text = self.first_text().unwrap_or_else(|| {
            panic!("Expected resource result to contain text, but no text content found")
        });
        assert!(
            text.contains(substring),
            "Expected resource result to contain '{}', but got '{}'",
            substring,
            text
        );
    }
}

/// Extension trait for PromptResult assertions.
pub trait PromptResultAssertions {
    /// Get the first message content, if any.
    fn first_message_text(&self) -> Option<String>;

    /// Assert the result has at least one message.
    fn assert_has_messages(&self);

    /// Assert the result has the expected number of messages.
    fn assert_message_count(&self, expected: usize);

    /// Assert the first message contains the substring.
    fn assert_first_message_contains(&self, substring: &str);
}

impl PromptResultAssertions for PromptResult {
    fn first_message_text(&self) -> Option<String> {
        self.messages
            .first()
            .and_then(|m| m.content.as_text().map(|s| s.to_string()))
    }

    fn assert_has_messages(&self) {
        assert!(
            !self.messages.is_empty(),
            "Expected prompt result to have messages"
        );
    }

    fn assert_message_count(&self, expected: usize) {
        let actual = self.messages.len();
        assert_eq!(
            actual, expected,
            "Expected {} messages, found {}",
            expected, actual
        );
    }

    fn assert_first_message_contains(&self, substring: &str) {
        let text = self.first_message_text().unwrap_or_else(|| {
            panic!("Expected prompt result to have text message, but none found")
        });
        assert!(
            text.contains(substring),
            "Expected first message to contain '{}', but got '{}'",
            substring,
            text
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wasm_server::McpServer;
    use schemars::JsonSchema;
    use serde::Deserialize;

    #[derive(Deserialize, JsonSchema)]
    struct GreetArgs {
        name: String,
    }

    #[derive(Deserialize, JsonSchema)]
    struct AddArgs {
        a: i64,
        b: i64,
    }

    fn create_test_server() -> McpServer {
        McpServer::builder("test-server", "1.0.0")
            .description("A test server")
            .tool("greet", "Say hello", |args: GreetArgs| async move {
                format!("Hello, {}!", args.name)
            })
            .tool("add", "Add two numbers", |args: AddArgs| async move {
                format!("{}", args.a + args.b)
            })
            .build()
    }

    #[test]
    fn test_server_info() {
        let server = create_test_server();
        let client = McpTestClient::new(server);
        assert_eq!(client.server_name(), "test-server");
        assert_eq!(client.server_version(), "1.0.0");
    }

    #[test]
    fn test_list_tools() {
        let server = create_test_server();
        let client = McpTestClient::new(server);
        let tools = client.list_tools();
        assert_eq!(tools.len(), 2);
    }

    #[test]
    fn test_assert_tool_exists() {
        let server = create_test_server();
        let client = McpTestClient::new(server);
        let tool = client.assert_tool_exists("greet");
        assert_eq!(tool.name, "greet");
    }

    #[test]
    #[should_panic(expected = "Tool 'nonexistent' not found")]
    fn test_assert_tool_exists_fails() {
        let server = create_test_server();
        let client = McpTestClient::new(server);
        client.assert_tool_exists("nonexistent");
    }

    #[test]
    fn test_assert_tool_not_exists() {
        let server = create_test_server();
        let client = McpTestClient::new(server);
        client.assert_tool_not_exists("nonexistent");
    }

    #[test]
    fn test_assert_tool_count() {
        let server = create_test_server();
        let client = McpTestClient::new(server);
        client.assert_tool_count(2);
    }

    #[test]
    fn test_with_session() {
        let server = create_test_server();
        let client = McpTestClient::new(server).with_session("test-session-123");
        // Session is set for context creation
        assert_eq!(client.session_id, Some("test-session-123".to_string()));
    }

    #[test]
    fn test_with_user() {
        let server = create_test_server();
        let client = McpTestClient::new(server).with_user("user-456");
        assert_eq!(client.user_id, Some("user-456".to_string()));
    }

    #[test]
    fn test_with_roles() {
        let server = create_test_server();
        let client =
            McpTestClient::new(server).with_roles(vec!["admin".to_string(), "user".to_string()]);
        assert_eq!(client.roles, vec!["admin", "user"]);
    }
}
