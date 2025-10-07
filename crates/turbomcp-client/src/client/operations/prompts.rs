//! Prompt operations for MCP client
//!
//! This module provides prompt-related functionality including listing prompts,
//! retrieving prompt templates, and supporting parameter substitution.

use std::sync::atomic::Ordering;

use turbomcp_protocol::{Error, Result};
use turbomcp_protocol::types::{
    GetPromptRequest, GetPromptResult, ListPromptsResult, Prompt, PromptInput,
};

impl<T: turbomcp_transport::Transport> super::super::core::Client<T> {
    /// List available prompt templates from the server
    ///
    /// Retrieves the complete list of prompt templates that the server provides,
    /// including all metadata: title, description, and argument schemas. This is
    /// the MCP-compliant implementation that provides everything needed for UI generation
    /// and dynamic form creation.
    ///
    /// # Returns
    ///
    /// Returns a vector of `Prompt` objects containing:
    /// - `name`: Programmatic identifier
    /// - `title`: Human-readable display name (optional)
    /// - `description`: Description of what the prompt does (optional)
    /// - `arguments`: Array of argument schemas with validation info (optional)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client is not initialized
    /// - The server doesn't support prompts
    /// - The request fails
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
    /// let prompts = client.list_prompts().await?;
    /// for prompt in prompts {
    ///     println!("Prompt: {} ({})", prompt.name, prompt.title.unwrap_or("No title".to_string()));
    ///     if let Some(args) = prompt.arguments {
    ///         println!("  Arguments: {:?}", args);
    ///         for arg in args {
    ///             let required = arg.required.unwrap_or(false);
    ///             println!("    - {}: {} (required: {})", arg.name,
    ///                     arg.description.unwrap_or("No description".to_string()), required);
    ///         }
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn list_prompts(&self) -> Result<Vec<Prompt>> {
        if !self.inner.initialized.load(Ordering::Relaxed) {
            return Err(Error::bad_request("Client not initialized"));
        }

        // Execute with plugin middleware - return full Prompt objects per MCP spec
        let response: ListPromptsResult = self.execute_with_plugins("prompts/list", None).await?;
        Ok(response.prompts)
    }

    /// Get a specific prompt template with argument support
    ///
    /// Retrieves a specific prompt template from the server with support for
    /// parameter substitution. When arguments are provided, the server will
    /// substitute them into the prompt template using {parameter} syntax.
    ///
    /// This is the MCP-compliant implementation that supports the full protocol specification.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the prompt to retrieve
    /// * `arguments` - Optional parameters for template substitution
    ///
    /// # Returns
    ///
    /// Returns `GetPromptResult` containing the prompt template with parameters substituted.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client is not initialized
    /// - The prompt name is empty
    /// - The prompt doesn't exist
    /// - Required arguments are missing
    /// - Argument types don't match schema
    /// - The request fails
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::Client;
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # use turbomcp_protocol::PromptInput;
    /// # use std::collections::HashMap;
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// let mut client = Client::new(StdioTransport::new());
    /// client.initialize().await?;
    ///
    /// // Get prompt without arguments (template form)
    /// let template = client.get_prompt("greeting", None).await?;
    /// println!("Template has {} messages", template.messages.len());
    ///
    /// // Get prompt with arguments (substituted form)
    /// let mut args = HashMap::new();
    /// args.insert("name".to_string(), serde_json::Value::String("Alice".to_string()));
    /// args.insert("greeting".to_string(), serde_json::Value::String("Hello".to_string()));
    ///
    /// let result = client.get_prompt("greeting", Some(args)).await?;
    /// println!("Generated prompt with {} messages", result.messages.len());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_prompt(
        &self,
        name: &str,
        arguments: Option<PromptInput>,
    ) -> Result<GetPromptResult> {
        if !self.inner.initialized.load(Ordering::Relaxed) {
            return Err(Error::bad_request("Client not initialized"));
        }

        if name.is_empty() {
            return Err(Error::bad_request("Prompt name cannot be empty"));
        }

        // Send prompts/get request with full argument support
        let request = GetPromptRequest {
            name: name.to_string(),
            arguments, // Support for parameter substitution
            _meta: None,
        };

        self.execute_with_plugins(
            "prompts/get",
            Some(serde_json::to_value(request).map_err(|e| {
                Error::protocol(format!("Failed to serialize prompt request: {}", e))
            })?),
        )
        .await
    }
}
