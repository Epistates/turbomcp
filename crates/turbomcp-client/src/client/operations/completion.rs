//! Completion operations for MCP client
//!
//! This module provides autocompletion functionality for prompts and resources,
//! supporting the MCP completion protocol with context and argument validation.

use std::sync::atomic::Ordering;
use turbomcp_protocol::types::{
    ArgumentInfo, CompleteRequestParams, CompleteResult, CompletionContext, CompletionReference,
    CompletionResponse, PromptReferenceData, ResourceTemplateReferenceData,
};
use turbomcp_protocol::{Error, Result};

use crate::with_plugins;

impl<T: turbomcp_transport::Transport> super::super::core::Client<T> {
    /// Internal helper for completion operations - DRYed up common logic
    async fn complete_internal(
        &self,
        argument_name: &str,
        argument_value: &str,
        reference: CompletionReference,
        context: Option<CompletionContext>,
    ) -> Result<CompletionResponse> {
        if !self.inner.initialized.load(Ordering::Relaxed) {
            return Err(Error::bad_request("Client not initialized"));
        }

        let request_params = CompleteRequestParams {
            argument: ArgumentInfo {
                name: argument_name.to_string(),
                value: argument_value.to_string(),
            },
            reference,
            context,
        };

        let serialized_params = serde_json::to_value(&request_params)?;

        with_plugins!(self, "completion/complete", serialized_params, {
            let result: CompleteResult = self
                .inner
                .protocol
                .request("completion/complete", Some(serialized_params))
                .await?;

            Ok(CompletionResponse {
                completion: result.completion,
                _meta: result._meta,
            })
        })
    }

    /// Request completion suggestions from the server
    ///
    /// Simple completion interface for basic autocompletion needs.
    /// Uses a prompt-based reference with hardcoded "partial" argument name.
    ///
    /// # Arguments
    ///
    /// * `handler_name` - The completion handler name
    /// * `argument_value` - The partial value to complete
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
    /// let result = client.complete("complete_path", "/usr/b").await?;
    /// println!("Completions: {:?}", result.completion.values);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn complete(
        &self,
        handler_name: &str,
        argument_value: &str,
    ) -> Result<CompletionResponse> {
        let reference = CompletionReference::Prompt(PromptReferenceData {
            name: handler_name.to_string(),
            title: None,
        });

        self.complete_internal("partial", argument_value, reference, None)
            .await
    }

    /// Complete a prompt argument with full MCP protocol support
    ///
    /// This method provides access to the complete MCP completion protocol,
    /// allowing specification of argument names, prompt references, and context.
    ///
    /// # Arguments
    ///
    /// * `prompt_name` - Name of the prompt to complete for
    /// * `argument_name` - Name of the argument being completed
    /// * `argument_value` - Current value for completion matching
    /// * `context` - Optional context with previously resolved arguments
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::Client;
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # use turbomcp_protocol::types::CompletionContext;
    /// # use std::collections::HashMap;
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// let mut client = Client::new(StdioTransport::new());
    /// client.initialize().await?;
    ///
    /// // Complete with context
    /// let mut context_args = HashMap::new();
    /// context_args.insert("language".to_string(), "rust".to_string());
    /// let context = CompletionContext { arguments: Some(context_args) };
    ///
    /// let completions = client.complete_prompt(
    ///     "code_review",
    ///     "framework",
    ///     "tok",
    ///     Some(context)
    /// ).await?;
    ///
    /// for completion in completions.completion.values {
    ///     println!("Suggestion: {}", completion);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn complete_prompt(
        &self,
        prompt_name: &str,
        argument_name: &str,
        argument_value: &str,
        context: Option<CompletionContext>,
    ) -> Result<CompletionResponse> {
        let reference = CompletionReference::Prompt(PromptReferenceData {
            name: prompt_name.to_string(),
            title: None,
        });

        self.complete_internal(argument_name, argument_value, reference, context)
            .await
    }

    /// Complete a resource template URI with full MCP protocol support
    ///
    /// This method provides completion for resource template URIs, allowing
    /// servers to suggest values for URI template variables.
    ///
    /// # Arguments
    ///
    /// * `resource_uri` - Resource template URI (e.g., "/files/{path}")
    /// * `argument_name` - Name of the argument being completed
    /// * `argument_value` - Current value for completion matching
    /// * `context` - Optional context with previously resolved arguments
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
    /// let completions = client.complete_resource(
    ///     "/files/{path}",
    ///     "path",
    ///     "/home/user/doc",
    ///     None
    /// ).await?;
    ///
    /// for completion in completions.completion.values {
    ///     println!("Path suggestion: {}", completion);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn complete_resource(
        &self,
        resource_uri: &str,
        argument_name: &str,
        argument_value: &str,
        context: Option<CompletionContext>,
    ) -> Result<CompletionResponse> {
        let reference = CompletionReference::ResourceTemplate(ResourceTemplateReferenceData {
            uri: resource_uri.to_string(),
        });

        self.complete_internal(argument_name, argument_value, reference, context)
            .await
    }
}
