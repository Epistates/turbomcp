//! Server builder pattern for MCP server construction
//!
//! Provides a fluent builder API for configuring and constructing MCP servers
//! with handlers, configuration, and filesystem roots.

use crate::{
    config::ServerConfig,
    error::ServerResult,
    handlers::{PromptHandler, ResourceHandler, ToolHandler},
    registry::HandlerRegistry,
};

use super::core::McpServer;

/// Builder for constructing MCP servers with configuration and handlers
pub struct ServerBuilder {
    /// Server configuration
    config: ServerConfig,
    /// Registry builder
    registry: HandlerRegistry,
}

impl std::fmt::Debug for ServerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerBuilder")
            .field("config", &self.config)
            .finish()
    }
}

impl ServerBuilder {
    /// Create a new server builder
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: ServerConfig::default(),
            registry: HandlerRegistry::new(),
        }
    }

    /// Set server name
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.config.name = name.into();
        self
    }

    /// Set server version
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.config.version = version.into();
        self
    }

    /// Set server description
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.config.description = Some(description.into());
        self
    }

    /// Add a tool handler
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Handler`] if:
    /// - The handler limit is exceeded
    /// - Handler validation fails
    /// - A handler with the same name already exists
    pub fn tool<T>(self, name: impl Into<String>, handler: T) -> ServerResult<Self>
    where
        T: ToolHandler + 'static,
    {
        self.registry.register_tool(name, handler)?;
        Ok(self)
    }

    /// Add a prompt handler
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Handler`] if:
    /// - The handler limit is exceeded
    /// - Handler validation fails
    /// - A handler with the same name already exists
    pub fn prompt<P>(self, name: impl Into<String>, handler: P) -> ServerResult<Self>
    where
        P: PromptHandler + 'static,
    {
        self.registry.register_prompt(name, handler)?;
        Ok(self)
    }

    /// Add a resource handler
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Handler`] if:
    /// - The handler limit is exceeded
    /// - Handler validation fails
    /// - A handler with the same URI already exists
    pub fn resource<R>(self, name: impl Into<String>, handler: R) -> ServerResult<Self>
    where
        R: ResourceHandler + 'static,
    {
        self.registry.register_resource(name, handler)?;
        Ok(self)
    }

    /// Add a filesystem root
    pub fn root(self, uri: impl Into<String>, name: Option<String>) -> Self {
        use turbomcp_protocol::types::Root;
        self.registry.add_root(Root {
            uri: uri.into(),
            name,
        });
        self
    }

    /// Set multiple filesystem roots
    pub fn roots(self, roots: Vec<turbomcp_protocol::types::Root>) -> Self {
        self.registry.set_roots(roots);
        self
    }

    /// Build the server
    #[must_use]
    pub fn build(self) -> McpServer {
        // Build server with correct registry from the start
        // This ensures the Tower service stack uses the populated registry
        McpServer::new_with_registry(self.config, self.registry)
    }
}

impl Default for ServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}
