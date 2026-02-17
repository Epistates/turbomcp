//! Tool operations for MCP client
//!
//! This module provides tool-related functionality including listing tools,
//! calling tools, and processing tool results.

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use turbomcp_protocol::types::{CallToolRequest, CallToolResult, ListToolsResult, Tool};
use turbomcp_protocol::{Error, Result};

impl<T: turbomcp_transport::Transport + 'static> super::super::core::Client<T> {
    /// List all available tools from the MCP server
    ///
    /// Returns complete tool definitions with schemas that can be used
    /// for form generation, validation, and documentation. Tools represent
    /// executable functions provided by the server.
    ///
    /// # Returns
    ///
    /// Returns a vector of Tool objects with complete metadata including names,
    /// descriptions, and input schemas. These schemas can be used to generate
    /// user interfaces for tool invocation.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::Client;
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// let mut client = Client::new(StdioTransport::new());
    /// client.initialize().await?;
    ///
    /// let tools = client.list_tools().await?;
    /// for tool in tools {
    ///     println!("Tool: {} - {}", tool.name, tool.description.as_deref().unwrap_or("No description"));
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn list_tools(&self) -> Result<Vec<Tool>> {
        if !self.inner.initialized.load(Ordering::Relaxed) {
            return Err(Error::invalid_request("Client not initialized"));
        }

        // Send tools/list request
        let response: ListToolsResult = self.inner.protocol.request("tools/list", None).await?;
        Ok(response.tools) // Return full Tool objects with schemas
    }

    /// List available tool names from the MCP server
    ///
    /// Returns only the tool names for cases where full schemas are not needed.
    /// For most use cases, prefer `list_tools()` which provides complete tool definitions.
    ///
    /// # Returns
    ///
    /// Returns a vector of tool names available on the server.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::Client;
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// let mut client = Client::new(StdioTransport::new());
    /// client.initialize().await?;
    ///
    /// let tool_names = client.list_tool_names().await?;
    /// for name in tool_names {
    ///     println!("Available tool: {}", name);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn list_tool_names(&self) -> Result<Vec<String>> {
        let tools = self.list_tools().await?;
        Ok(tools.into_iter().map(|tool| tool.name).collect())
    }

    /// Call a tool on the server
    ///
    /// Executes a tool on the server with the provided arguments and returns
    /// the complete MCP `CallToolResult`.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the tool to call
    /// * `arguments` - Optional arguments to pass to the tool
    ///
    /// # Returns
    ///
    /// Returns the complete `CallToolResult` with:
    /// - `content: Vec<ContentBlock>` - All content blocks (text, image, resource, audio, etc.)
    /// - `is_error: Option<bool>` - Whether the tool execution resulted in an error
    /// - `structured_content: Option<serde_json::Value>` - Schema-validated structured output
    /// - `_meta: Option<serde_json::Value>` - Metadata for client applications (not exposed to LLMs)
    ///
    /// # Examples
    ///
    /// ## Basic Usage
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::Client;
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # use turbomcp_protocol::types::Content;
    /// # use std::collections::HashMap;
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// let mut client = Client::new(StdioTransport::new());
    /// client.initialize().await?;
    ///
    /// let mut args = HashMap::new();
    /// args.insert("input".to_string(), serde_json::json!("test"));
    ///
    /// let result = client.call_tool("my_tool", Some(args)).await?;
    ///
    /// // Access all content blocks
    /// for content in &result.content {
    ///     match content {
    ///         Content::Text(text) => println!("Text: {}", text.text),
    ///         Content::Image(image) => println!("Image: {}", image.mime_type),
    ///         _ => {}
    ///     }
    /// }
    ///
    /// // Check for errors
    /// if result.is_error.unwrap_or(false) {
    ///     eprintln!("Tool execution failed");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## Structured Output (Schema Validation)
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::Client;
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # use serde::Deserialize;
    /// # use std::collections::HashMap;
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// # #[derive(Deserialize)]
    /// # struct WeatherData {
    /// #     temperature: f64,
    /// #     conditions: String,
    /// # }
    /// let mut client = Client::new(StdioTransport::new());
    /// client.initialize().await?;
    ///
    /// let result = client.call_tool("get_weather", None).await?;
    ///
    /// // Access schema-validated structured output
    /// if let Some(structured) = result.structured_content {
    ///     let weather: WeatherData = serde_json::from_value(structured)?;
    ///     println!("Temperature: {}°C", weather.temperature);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## Metadata Access
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::Client;
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # use std::collections::HashMap;
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// let mut client = Client::new(StdioTransport::new());
    /// client.initialize().await?;
    ///
    /// let result = client.call_tool("query_database", None).await?;
    ///
    /// // Access metadata (tracking IDs, performance metrics, etc.)
    /// if let Some(meta) = result._meta {
    ///     if let Some(query_id) = meta.get("query_id") {
    ///         println!("Query ID: {}", query_id);
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Option<HashMap<String, serde_json::Value>>,
    ) -> Result<CallToolResult> {
        if !self.inner.initialized.load(Ordering::Relaxed) {
            return Err(Error::invalid_request("Client not initialized"));
        }

        let request_data = CallToolRequest {
            name: name.to_string(),
            arguments: Some(arguments.unwrap_or_default()),
            _meta: None,
            ..Default::default()
        };

        // Core protocol call
        let result: CallToolResult = self
            .inner
            .protocol
            .request("tools/call", Some(serde_json::to_value(&request_data)?))
            .await?;

        Ok(result) // Return full CallToolResult - MCP spec compliant!
    }
}
