//! Resource operations for MCP client
//!
//! This module provides resource-related functionality including listing resources,
//! reading resource content, and managing resource templates.

use std::sync::atomic::Ordering;

use turbomcp_protocol::types::{
    Cursor, ListResourceTemplatesRequest, ListResourceTemplatesResult, ListResourcesRequest,
    ListResourcesResult, ReadResourceRequest, ReadResourceResult, Resource, ResourceTemplate,
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
            let result = self.list_resources_paginated(cursor.clone()).await?;
            all_resources.extend(result.resources);
            match result.next_cursor {
                // `nextCursor` is the only end-of-results signal the spec
                // defines. An empty page that still carries one is legal — a
                // server may filter a page down to nothing — and stopping
                // there silently truncated the list. The second guard catches
                // a server that repeats a cursor forever, which would
                // otherwise spin to the page cap.
                Some(next) if Some(&next) != cursor.as_ref() => cursor = Some(next),
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
        self.request("resources/list", params).await
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
    /// Returns a vector of resource templates with their full metadata.
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
    ///     println!("Resource template: {}", template.uri_template);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn list_resource_templates(&self) -> Result<Vec<ResourceTemplate>> {
        if !self.inner.initialized.load(Ordering::Relaxed) {
            return Err(Error::invalid_request("Client not initialized"));
        }

        let mut all_templates = Vec::new();
        let mut cursor = None;
        for _ in 0..MAX_PAGINATION_PAGES {
            let result = self
                .list_resource_templates_paginated(cursor.clone())
                .await?;
            all_templates.extend(result.resource_templates);
            match result.next_cursor {
                // `nextCursor` is the only end-of-results signal the spec
                // defines. An empty page that still carries one is legal — a
                // server may filter a page down to nothing — and stopping
                // there silently truncated the list. The second guard catches
                // a server that repeats a cursor forever, which would
                // otherwise spin to the page cap.
                Some(next) if Some(&next) != cursor.as_ref() => cursor = Some(next),
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
        self.request("resources/templates/list", params).await
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::core::Client;
    use super::*;
    use std::collections::VecDeque;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Mutex;
    use turbomcp_protocol::MessageId;
    use turbomcp_transport::{
        Transport, TransportCapabilities, TransportError, TransportMessage, TransportMetrics,
        TransportResult, TransportState, TransportType,
    };

    #[derive(Debug)]
    struct TemplateTransport {
        capabilities: TransportCapabilities,
        responses: Mutex<VecDeque<TransportMessage>>,
    }

    impl TemplateTransport {
        fn new() -> Self {
            Self {
                capabilities: TransportCapabilities::default(),
                responses: Mutex::new(VecDeque::new()),
            }
        }
    }

    impl Transport for TemplateTransport {
        fn transport_type(&self) -> TransportType {
            TransportType::Stdio
        }

        fn capabilities(&self) -> &TransportCapabilities {
            &self.capabilities
        }

        fn state(&self) -> Pin<Box<dyn Future<Output = TransportState> + Send + '_>> {
            Box::pin(async { TransportState::Connected })
        }

        fn connect(&self) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }

        fn disconnect(&self) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }

        fn send(
            &self,
            message: TransportMessage,
        ) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
            let request: serde_json::Value = match serde_json::from_slice(&message.payload) {
                Ok(request) => request,
                Err(e) => {
                    return Box::pin(async move {
                        Err(TransportError::SerializationFailed(e.to_string()))
                    });
                }
            };

            assert_eq!(request["method"], "resources/templates/list");
            let response = serde_json::json!({
                "jsonrpc": "2.0",
                "id": request["id"].clone(),
                "result": {
                    "resourceTemplates": [
                        {
                            "uriTemplate": "repo://{owner}/{name}",
                            "name": "repo",
                            "title": "Repository",
                            "description": "Repository metadata",
                            "mimeType": "application/json",
                            "icons": [
                                {
                                    "src": "https://example.com/repo.png",
                                    "mimeType": "image/png",
                                    "sizes": ["64x64"]
                                }
                            ],
                            "annotations": {
                                "audience": ["user"],
                                "priority": 0.7,
                                "lastModified": "2026-05-08T12:00:00Z"
                            },
                            "_meta": {
                                "x-test": true
                            }
                        }
                    ]
                }
            });
            let payload = match serde_json::to_vec(&response) {
                Ok(payload) => payload,
                Err(e) => {
                    return Box::pin(async move {
                        Err(TransportError::SerializationFailed(e.to_string()))
                    });
                }
            };
            self.responses
                .lock()
                .expect("response queue poisoned")
                .push_back(TransportMessage::new(
                    MessageId::from("response-1"),
                    payload.into(),
                ));
            Box::pin(async { Ok(()) })
        }

        fn receive(
            &self,
        ) -> Pin<Box<dyn Future<Output = TransportResult<Option<TransportMessage>>> + Send + '_>>
        {
            let response = self
                .responses
                .lock()
                .expect("response queue poisoned")
                .pop_front();
            Box::pin(async move { Ok(response) })
        }

        fn metrics(&self) -> Pin<Box<dyn Future<Output = TransportMetrics> + Send + '_>> {
            Box::pin(async { TransportMetrics::default() })
        }
    }

    #[tokio::test]
    async fn list_resource_templates_preserves_full_template_metadata() {
        let client = Client::new(TemplateTransport::new());
        client.inner.initialized.store(true, Ordering::Relaxed);

        let templates = client
            .list_resource_templates()
            .await
            .expect("resource templates");

        assert_eq!(templates.len(), 1);
        let template = &templates[0];
        assert_eq!(template.uri_template, "repo://{owner}/{name}");
        assert_eq!(template.name, "repo");
        assert_eq!(template.title.as_deref(), Some("Repository"));
        assert_eq!(template.description.as_deref(), Some("Repository metadata"));
        assert_eq!(template.mime_type.as_deref(), Some("application/json"));
        assert_eq!(
            template.icons.as_ref().expect("icons")[0].src,
            "https://example.com/repo.png"
        );
        assert_eq!(
            template.annotations.as_ref().expect("annotations").priority,
            Some(0.7)
        );
        assert_eq!(
            template.meta.as_ref().expect("meta")["x-test"],
            serde_json::json!(true)
        );
    }
}
