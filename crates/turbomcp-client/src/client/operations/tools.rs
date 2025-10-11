//! Tool operations for MCP client
//!
//! This module provides tool-related functionality including listing tools,
//! calling tools, and processing tool results.

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use turbomcp_protocol::types::{CallToolRequest, CallToolResult, Content, ListToolsResult, Tool};
use turbomcp_protocol::{Error, Result};

use crate::with_plugins;

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
            return Err(Error::bad_request("Client not initialized"));
        }

        // Send tools/list request with plugin middleware
        let response: ListToolsResult = self.execute_with_plugins("tools/list", None).await?;
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
    /// Executes a tool on the server with the provided arguments.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the tool to call
    /// * `arguments` - Optional arguments to pass to the tool
    ///
    /// # Returns
    ///
    /// Returns the result of the tool execution.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::Client;
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # use std::collections::HashMap;
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// let mut client = Client::new(StdioTransport::new());
    /// client.initialize().await?;
    ///
    /// let mut args = HashMap::new();
    /// args.insert("input".to_string(), serde_json::json!("test"));
    ///
    /// let result = client.call_tool("my_tool", Some(args)).await?;
    /// println!("Tool result: {:?}", result);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Option<HashMap<String, serde_json::Value>>,
    ) -> Result<serde_json::Value> {
        if !self.inner.initialized.load(Ordering::Relaxed) {
            return Err(Error::bad_request("Client not initialized"));
        }

        // 🎉 TurboMCP v1.0.7: Clean plugin execution with macro!
        let request_data = CallToolRequest {
            name: name.to_string(),
            arguments: Some(arguments.unwrap_or_default()),
            _meta: None,
        };

        with_plugins!(self, "tools/call", request_data, {
            // Core protocol call - plugins execute automatically around this
            let result: CallToolResult = self
                .inner
                .protocol
                .request("tools/call", Some(serde_json::to_value(&request_data)?))
                .await?;

            Ok(self.extract_tool_content(&result))
        })
    }

    /// Helper method to extract content from CallToolResult
    fn extract_tool_content(&self, response: &CallToolResult) -> serde_json::Value {
        // Extract content from response - for simplicity, return the first text content
        if let Some(content) = response.content.first() {
            match content {
                Content::Text(text_content) => serde_json::json!({
                    "text": text_content.text,
                    "is_error": response.is_error.unwrap_or(false)
                }),
                Content::Image(image_content) => serde_json::json!({
                    "image": image_content.data,
                    "mime_type": image_content.mime_type,
                    "is_error": response.is_error.unwrap_or(false)
                }),
                Content::Resource(resource_content) => serde_json::json!({
                    "resource": resource_content.resource,
                    "annotations": resource_content.annotations,
                    "is_error": response.is_error.unwrap_or(false)
                }),
                Content::Audio(audio_content) => serde_json::json!({
                    "audio": audio_content.data,
                    "mime_type": audio_content.mime_type,
                    "is_error": response.is_error.unwrap_or(false)
                }),
                Content::ResourceLink(resource_link) => serde_json::json!({
                    "resource_uri": resource_link.uri,
                    "is_error": response.is_error.unwrap_or(false)
                }),
            }
        } else {
            serde_json::json!({
                "message": "No content returned",
                "is_error": response.is_error.unwrap_or(false)
            })
        }
    }
}
