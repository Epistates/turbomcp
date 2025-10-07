//! Resource operations for MCP client
//!
//! This module provides resource-related functionality including listing resources,
//! reading resource content, and managing resource templates.

use std::sync::atomic::Ordering;

use turbomcp_protocol::{Error, Result};
use turbomcp_protocol::types::{
    ListResourceTemplatesResult, ListResourcesResult, ReadResourceRequest, ReadResourceResult,
};

impl<T: turbomcp_transport::Transport> super::super::core::Client<T> {
    /// List available resources from the MCP server
    ///
    /// Returns a list of resource URIs that are available for reading.
    /// Resources represent data or content that can be accessed by the client.
    ///
    /// # Returns
    ///
    /// Returns a vector of resource URIs that can be read using `read_resource()`.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client is not initialized
    /// - The server doesn't support resources
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
    /// let resources = client.list_resources().await?;
    /// for resource in resources {
    ///     println!("Available resource: {}", resource);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn list_resources(&self) -> Result<Vec<String>> {
        if !self.inner.initialized.load(Ordering::Relaxed) {
            return Err(Error::bad_request("Client not initialized"));
        }

        // Execute with plugin middleware
        let response: ListResourcesResult =
            self.execute_with_plugins("resources/list", None).await?;

        let resource_uris = response
            .resources
            .into_iter()
            .map(|resource| resource.uri)
            .collect();
        Ok(resource_uris)
    }

    /// Read the content of a specific resource by URI
    ///
    /// Retrieves the content of a resource identified by its URI.
    /// Resources can contain text, binary data, or structured content.
    ///
    /// # Arguments
    ///
    /// * `uri` - The URI of the resource to read
    ///
    /// # Returns
    ///
    /// Returns `ReadResourceResult` containing the resource content and metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client is not initialized
    /// - The URI is empty or invalid
    /// - The resource doesn't exist
    /// - Access to the resource is denied
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
    /// let result = client.read_resource("file:///path/to/document.txt").await?;
    /// for content in result.contents {
    ///     println!("Resource content: {:?}", content);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn read_resource(&self, uri: &str) -> Result<ReadResourceResult> {
        if !self.inner.initialized.load(Ordering::Relaxed) {
            return Err(Error::bad_request("Client not initialized"));
        }

        if uri.is_empty() {
            return Err(Error::bad_request("Resource URI cannot be empty"));
        }

        // Send read_resource request
        let request = ReadResourceRequest {
            uri: uri.to_string(),
            _meta: None,
        };

        let response: ReadResourceResult = self
            .execute_with_plugins("resources/read", Some(serde_json::to_value(request)?))
            .await?;
        Ok(response)
    }

    /// List available resource templates from the MCP server
    ///
    /// Returns a list of resource template URIs that define patterns for
    /// generating resource URIs. Templates allow servers to describe
    /// families of related resources without listing each individual resource.
    ///
    /// # Returns
    ///
    /// Returns a vector of resource template URI patterns.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client is not initialized
    /// - The server doesn't support resource templates
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
    /// let templates = client.list_resource_templates().await?;
    /// for template in templates {
    ///     println!("Resource template: {}", template);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn list_resource_templates(&self) -> Result<Vec<String>> {
        if !self.inner.initialized.load(Ordering::Relaxed) {
            return Err(Error::bad_request("Client not initialized"));
        }

        // Send resources/templates request with plugin middleware
        let response: ListResourceTemplatesResult = self
            .execute_with_plugins("resources/templates", None)
            .await?;
        let template_uris = response
            .resource_templates
            .into_iter()
            .map(|template| template.uri_template)
            .collect();
        Ok(template_uris)
    }
}
