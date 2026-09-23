//! MCP Server Introspector
//!
//! This module provides the core introspection logic for discovering MCP server
//! capabilities by communicating via the MCP protocol.

use serde::de::DeserializeOwned;
use tracing::{debug, info, trace};
use turbomcp_protocol::{
    InitializeRequest, InitializeResult, PROTOCOL_VERSION,
    types::{
        ClientCapabilities, Cursor, Implementation, ListPromptsResult, ListResourceTemplatesResult,
        ListResourcesResult, ListToolsResult,
    },
};

use super::backends::McpBackend;
use super::spec::ServerSpec;
use crate::error::{ProxyError, ProxyResult};

/// Upper bound on pages walked when introspecting a backend.
///
/// The backend is a foreign server: it may repeat a cursor, or hand out a new
/// one forever. Without a bound, introspection is a memory and time sink
/// controlled entirely by the other side. 1000 pages is far past any real
/// catalogue while still terminating.
const MAX_PAGINATION_PAGES: usize = 1000;

/// MCP Server Introspector
///
/// Discovers server capabilities by performing MCP protocol handshake
/// and listing all available tools, resources, and prompts.
pub struct McpIntrospector {
    /// Client name to send during initialization
    client_name: String,
    /// Client version
    client_version: String,
}

impl McpIntrospector {
    /// Create a new introspector with default client info
    #[must_use]
    pub fn new() -> Self {
        Self {
            client_name: "turbomcp-proxy-introspector".to_string(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Create an introspector with custom client info
    pub fn with_client_info(
        client_name: impl Into<String>,
        client_version: impl Into<String>,
    ) -> Self {
        Self {
            client_name: client_name.into(),
            client_version: client_version.into(),
        }
    }

    /// Perform full introspection of an MCP server
    ///
    /// This will:
    /// 1. Connect to the server via the backend
    /// 2. Perform initialization handshake
    /// 3. List all tools, resources, and prompts
    /// 4. Build a complete `ServerSpec`
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if connection fails, initialization fails, or listing resources fails.
    pub async fn introspect(&self, backend: &mut dyn McpBackend) -> ProxyResult<ServerSpec> {
        info!(
            client = %self.client_name,
            version = %self.client_version,
            backend = %backend.description(),
            "Starting MCP server introspection"
        );

        // Step 1: Initialize connection
        let init_result = self.initialize(backend).await?;

        debug!(
            server_name = %init_result.server_info.name,
            server_version = %init_result.server_info.version,
            protocol_version = %init_result.protocol_version,
            "Server initialization successful"
        );

        let capabilities = init_result.capabilities;

        // Step 2: List each family the server declared. MCP §Operation makes
        // "only use capabilities that were successfully negotiated" a MUST.
        let tools = if capabilities.tools.is_some() {
            list_all(backend, "tools/list", |page: ListToolsResult| {
                (page.tools, page.next_cursor)
            })
            .await?
        } else {
            debug!("Server does not support tools");
            Vec::new()
        };

        let (resources, resource_templates) = if capabilities.resources.is_some() {
            let resources = list_all(backend, "resources/list", |page: ListResourcesResult| {
                (page.resources, page.next_cursor)
            })
            .await?;
            let templates = list_all(
                backend,
                "resources/templates/list",
                |page: ListResourceTemplatesResult| (page.resource_templates, page.next_cursor),
            )
            .await?;
            (resources, templates)
        } else {
            debug!("Server does not support resources");
            (Vec::new(), Vec::new())
        };

        let prompts = if capabilities.prompts.is_some() {
            list_all(backend, "prompts/list", |page: ListPromptsResult| {
                (page.prompts, page.next_cursor)
            })
            .await?
        } else {
            debug!("Server does not support prompts");
            Vec::new()
        };

        let spec = ServerSpec {
            server_info: init_result.server_info,
            protocol_version: init_result.protocol_version.to_string(),
            capabilities,
            tools,
            resources,
            resource_templates,
            prompts,
            instructions: init_result.instructions,
        };

        info!(
            server = %spec.server_info.name,
            tools = spec.tools.len(),
            resources = spec.resources.len(),
            prompts = spec.prompts.len(),
            "Introspection complete"
        );

        Ok(spec)
    }

    /// Initialize connection with the server
    async fn initialize(&self, backend: &mut dyn McpBackend) -> ProxyResult<InitializeResult> {
        let request = InitializeRequest {
            protocol_version: PROTOCOL_VERSION.into(),
            // Introspection only reads the catalogue, so it declares nothing.
            // Declaring roots, sampling, or elicitation invites the server to
            // send requests for them, and an introspector has no model, no
            // user, and no filesystem to answer with.
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: self.client_name.clone(),
                version: self.client_version.clone(),
                ..Default::default()
            },
            meta: None,
        };

        backend.initialize(request).await
    }
}

/// Walk every page of a list method.
///
/// Stops when the server omits `nextCursor`, the only end-of-results signal
/// the spec defines, or repeats the cursor it was just given, which would
/// otherwise spin to the page cap. An empty page that still carries a cursor
/// is legal and is followed.
async fn list_all<P, T>(
    backend: &mut dyn McpBackend,
    method: &str,
    split: impl Fn(P) -> (Vec<T>, Option<Cursor>),
) -> ProxyResult<Vec<T>>
where
    P: DeserializeOwned,
{
    let mut items = Vec::new();
    let mut cursor: Option<Cursor> = None;

    for _ in 0..MAX_PAGINATION_PAGES {
        trace!(method, cursor = ?cursor, "Fetching page");

        let params = match &cursor {
            Some(cursor) => serde_json::json!({ "cursor": cursor }),
            None => serde_json::json!({}),
        };
        let result = backend.call_method(method, params).await?;
        let page: P = serde_json::from_value(result)
            .map_err(|e| ProxyError::backend(format!("Failed to parse {method} response: {e}")))?;

        let (page_items, next) = split(page);
        items.extend(page_items);
        match next {
            Some(next) if Some(&next) != cursor.as_ref() => cursor = Some(next),
            _ => break,
        }
    }

    debug!(method, count = items.len(), "Listed all pages");
    Ok(items)
}

impl Default for McpIntrospector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_introspector_creation() {
        let introspector = McpIntrospector::new();
        assert_eq!(introspector.client_name, "turbomcp-proxy-introspector");

        let custom = McpIntrospector::with_client_info("my-client", "2.0.0");
        assert_eq!(custom.client_name, "my-client");
        assert_eq!(custom.client_version, "2.0.0");
    }
}
