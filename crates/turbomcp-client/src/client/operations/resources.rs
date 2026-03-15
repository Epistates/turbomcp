//! Resource operations for MCP client
//!
//! This module provides resource-related functionality including listing resources,
//! reading resource content, and managing resource templates.

use std::sync::atomic::Ordering;

use turbomcp_protocol::types::{
    Cursor, ListResourceTemplatesRequest, ListResourceTemplatesResult, ListResourcesRequest,
    ListResourcesResult, ReadResourceRequest, ReadResourceResult, Resource,
};
use turbomcp_protocol::{Error, Result};

/// Maximum number of pagination pages to prevent infinite loops from misbehaving servers.
const MAX_PAGINATION_PAGES: usize = 1000;

impl<T: turbomcp_transport::Transport + 'static> super::super::core::Client<T> {
    /// List available resources from the MCP server
    ///
    /// Returns a list of resources with their full metadata including URIs, names,
    /// descriptions, MIME types, and other attributes provided by the server.
    /// Resources represent data or content that can be accessed by the client.
    ///
    /// # Returns
    ///
    /// Returns a vector of `Resource` objects containing full metadata that can be
    /// read using `read_resource()`.
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
    ///     println!("Resource: {} ({})", resource.name, resource.uri);
    ///     if let Some(desc) = &resource.description {
    ///         println!("  Description: {}", desc);
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn list_resources(&self) -> Result<Vec<Resource>> {
        if !self.inner.initialized.load(Ordering::Relaxed) {
            return Err(Error::invalid_request("Client not initialized"));
        }

        let mut all_resources = Vec::new();
        let mut cursor = None;
        for _ in 0..MAX_PAGINATION_PAGES {
            let result = self.list_resources_paginated(cursor).await?;
            let page_empty = result.resources.is_empty();
            all_resources.extend(result.resources);
            match result.next_cursor {
                Some(c) if !page_empty => cursor = Some(c),
                _ => break,
            }
        }
        Ok(all_resources)
    }

    /// List resources with pagination support
    ///
    /// Returns the full `ListResourcesResult` including `next_cursor` for manual
    /// pagination control. Use `list_resources()` for automatic pagination.
    ///
    /// # Arguments
    ///
    /// * `cursor` - Optional cursor from a previous `ListResourcesResult::next_cursor`
    pub async fn list_resources_paginated(
        &self,
        cursor: Option<Cursor>,
    ) -> Result<ListResourcesResult> {
        if !self.inner.initialized.load(Ordering::Relaxed) {
            return Err(Error::invalid_request("Client not initialized"));
        }

        let request = ListResourcesRequest {
            cursor,
            _meta: None,
        };
        let params = if request.cursor.is_some() {
            Some(serde_json::to_value(&request)?)
        } else {
            None
        };
        self.inner.protocol.request("resources/list", params).await
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
            return Err(Error::invalid_request("Client not initialized"));
        }

        if uri.is_empty() {
            return Err(Error::invalid_request("Resource URI cannot be empty"));
        }

        // Send read_resource request
        let request = ReadResourceRequest {
            uri: uri.into(),
            _meta: None,
        };

        let response: ReadResourceResult = self
            .inner
            .protocol
            .request("resources/read", Some(serde_json::to_value(request)?))
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
            return Err(Error::invalid_request("Client not initialized"));
        }

        let mut all_templates = Vec::new();
        let mut cursor = None;
        for _ in 0..MAX_PAGINATION_PAGES {
            let result = self.list_resource_templates_paginated(cursor).await?;
            let page_empty = result.resource_templates.is_empty();
            all_templates.extend(
                result
                    .resource_templates
                    .into_iter()
                    .map(|t| t.uri_template),
            );
            match result.next_cursor {
                Some(c) if !page_empty => cursor = Some(c),
                _ => break,
            }
        }
        Ok(all_templates)
    }

    /// List resource templates with pagination support
    ///
    /// Returns the full `ListResourceTemplatesResult` including `next_cursor`
    /// for manual pagination control. Use `list_resource_templates()` for
    /// automatic pagination.
    ///
    /// # Arguments
    ///
    /// * `cursor` - Optional cursor from a previous result's `next_cursor`
    pub async fn list_resource_templates_paginated(
        &self,
        cursor: Option<Cursor>,
    ) -> Result<ListResourceTemplatesResult> {
        if !self.inner.initialized.load(Ordering::Relaxed) {
            return Err(Error::invalid_request("Client not initialized"));
        }

        let request = ListResourceTemplatesRequest {
            cursor,
            _meta: None,
        };
        let params = if request.cursor.is_some() {
            Some(serde_json::to_value(&request)?)
        } else {
            None
        };
        self.inner
            .protocol
            .request("resources/templates", params)
            .await
    }
}
