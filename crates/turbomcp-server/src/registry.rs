//! Handler registry for dynamic loading and management
//!
//! The registry provides centralized management of MCP handlers including tools,
//! prompts, resources, sampling, and logging handlers.
//!
//! # Examples
//!
//! ## Creating a registry
//!
//! ```
//! use turbomcp_server::registry::HandlerRegistry;
//!
//! let registry = HandlerRegistry::new();
//! assert_eq!(registry.tools.len(), 0);
//! assert_eq!(registry.prompts.len(), 0);
//! ```
//!
//! ## Working with registry configuration
//!
//! ```
//! use turbomcp_server::registry::RegistryConfig;
//!
//! let config = RegistryConfig::default();
//! assert_eq!(config.max_handlers_per_type, 1000);
//! assert!(config.enable_metrics);
//! assert!(config.enable_validation);
//!
//! let custom_config = RegistryConfig {
//!     max_handlers_per_type: 100,
//!     enable_metrics: false,
//!     enable_validation: true,
//!     handler_timeout_ms: 15_000,
//!     enable_hot_reload: false,
//!     event_listeners: vec!["audit".to_string()],
//! };
//! assert_eq!(custom_config.max_handlers_per_type, 100);
//! ```

use dashmap::DashMap;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use turbomcp_protocol::types::{Prompt, Resource, Root, Tool};

use crate::handlers::{
    HandlerMetadata, LoggingHandler, PromptHandler, ResourceHandler, SamplingHandler, ToolHandler,
};
use crate::{ServerError, ServerResult};

/// Handler registry for managing all server handlers
pub struct HandlerRegistry {
    /// Tool handlers
    pub tools: DashMap<String, Arc<dyn ToolHandler + 'static>>,
    /// Prompt handlers  
    pub prompts: DashMap<String, Arc<dyn PromptHandler + 'static>>,
    /// Resource handlers
    pub resources: DashMap<String, Arc<dyn ResourceHandler + 'static>>,
    /// Sampling handlers
    pub sampling: DashMap<String, Arc<dyn SamplingHandler + 'static>>,
    /// Logging handlers
    pub logging: DashMap<String, Arc<dyn LoggingHandler + 'static>>,
    /// Filesystem roots
    pub roots: Arc<RwLock<Vec<Root>>>,
    /// Handler metadata
    metadata: DashMap<String, HandlerMetadata>,
    /// Registry configuration
    config: Arc<RwLock<RegistryConfig>>,
}

impl std::fmt::Debug for HandlerRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HandlerRegistry")
            .field("tools_count", &self.tools.len())
            .field("prompts_count", &self.prompts.len())
            .field("resources_count", &self.resources.len())
            .field("sampling_count", &self.sampling.len())
            .field("logging_count", &self.logging.len())
            .field("roots_count", &self.roots.read().len())
            .finish()
    }
}

/// Registry configuration
#[derive(Debug, Clone)]
pub struct RegistryConfig {
    /// Maximum number of handlers per type
    pub max_handlers_per_type: usize,
    /// Enable handler metrics
    pub enable_metrics: bool,
    /// Enable handler validation
    pub enable_validation: bool,
    /// Handler timeout in milliseconds
    pub handler_timeout_ms: u64,
    /// Enable hot reloading
    pub enable_hot_reload: bool,
    /// Registry event listeners
    pub event_listeners: Vec<String>,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            max_handlers_per_type: 1000,
            enable_metrics: true,
            enable_validation: true,
            handler_timeout_ms: 30_000,
            enable_hot_reload: false,
            event_listeners: Vec::new(),
        }
    }
}

/// Registry events
#[derive(Debug, Clone)]
pub enum RegistryEvent {
    /// Handler registered
    HandlerRegistered {
        /// Handler type
        handler_type: String,
        /// Handler name
        name: String,
        /// Registration timestamp
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Handler unregistered
    HandlerUnregistered {
        /// Handler type
        handler_type: String,
        /// Handler name
        name: String,
        /// Unregistration timestamp
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Handler updated
    HandlerUpdated {
        /// Handler type
        handler_type: String,
        /// Handler name
        name: String,
        /// Update timestamp
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// Registry cleared
    RegistryCleared {
        /// Clear timestamp
        timestamp: chrono::DateTime<chrono::Utc>,
    },
}

impl HandlerRegistry {
    /// Create a new handler registry
    ///
    /// # Examples
    ///
    /// ```
    /// use turbomcp_server::registry::HandlerRegistry;
    ///
    /// let registry = HandlerRegistry::new();
    ///
    /// // All collections start empty
    /// assert_eq!(registry.tools.len(), 0);
    /// assert_eq!(registry.prompts.len(), 0);
    /// assert_eq!(registry.resources.len(), 0);
    /// assert_eq!(registry.sampling.len(), 0);
    /// assert_eq!(registry.logging.len(), 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            tools: DashMap::new(),
            prompts: DashMap::new(),
            resources: DashMap::new(),
            sampling: DashMap::new(),
            logging: DashMap::new(),
            roots: Arc::new(RwLock::new(Vec::new())),
            metadata: DashMap::new(),
            config: Arc::new(RwLock::new(RegistryConfig::default())),
        }
    }

    /// Create a registry with configuration
    #[must_use]
    pub fn with_config(config: RegistryConfig) -> Self {
        Self {
            tools: DashMap::new(),
            prompts: DashMap::new(),
            resources: DashMap::new(),
            sampling: DashMap::new(),
            logging: DashMap::new(),
            roots: Arc::new(RwLock::new(Vec::new())),
            metadata: DashMap::new(),
            config: Arc::new(RwLock::new(config)),
        }
    }

    /// Register a tool handler
    pub fn register_tool<T>(&self, name: impl Into<String>, handler: T) -> ServerResult<()>
    where
        T: ToolHandler + 'static,
    {
        let name = name.into();

        // Check limits
        if self.tools.len() >= self.config.read().max_handlers_per_type {
            return Err(ServerError::handler(format!(
                "Maximum number of tool handlers ({}) exceeded",
                self.config.read().max_handlers_per_type
            )));
        }

        // Validate handler if enabled
        if self.config.read().enable_validation {
            self.validate_tool_handler(&handler)?;
        }

        // Register the handler
        self.tools.insert(name.clone(), Arc::new(handler));

        // Store metadata
        let metadata = HandlerMetadata {
            name: name.clone(),
            version: "1.0.0".to_string(),
            description: None,
            tags: vec!["tool".to_string()],
            created_at: chrono::Utc::now(),
            config: HashMap::new(),
            metrics_enabled: self.config.read().enable_metrics,
            rate_limit: None,
            allowed_roles: None,
        };
        self.metadata.insert(format!("tool:{name}"), metadata);

        tracing::info!("Registered tool handler: {}", name);
        Ok(())
    }

    /// Register a prompt handler
    pub fn register_prompt<P>(&self, name: impl Into<String>, handler: P) -> ServerResult<()>
    where
        P: PromptHandler + 'static,
    {
        let name = name.into();

        // Check limits
        if self.prompts.len() >= self.config.read().max_handlers_per_type {
            return Err(ServerError::handler(format!(
                "Maximum number of prompt handlers ({}) exceeded",
                self.config.read().max_handlers_per_type
            )));
        }

        // Validate handler if enabled
        if self.config.read().enable_validation {
            self.validate_prompt_handler(&handler)?;
        }

        // Register the handler
        self.prompts.insert(name.clone(), Arc::new(handler));

        // Store metadata
        let metadata = HandlerMetadata {
            name: name.clone(),
            version: "1.0.0".to_string(),
            description: None,
            tags: vec!["prompt".to_string()],
            created_at: chrono::Utc::now(),
            config: HashMap::new(),
            metrics_enabled: self.config.read().enable_metrics,
            rate_limit: None,
            allowed_roles: None,
        };
        self.metadata.insert(format!("prompt:{name}"), metadata);

        tracing::info!("Registered prompt handler: {}", name);
        Ok(())
    }

    /// Register a resource handler
    pub fn register_resource<R>(&self, name: impl Into<String>, handler: R) -> ServerResult<()>
    where
        R: ResourceHandler + 'static,
    {
        let name = name.into();

        // Check limits
        if self.resources.len() >= self.config.read().max_handlers_per_type {
            return Err(ServerError::handler(format!(
                "Maximum number of resource handlers ({}) exceeded",
                self.config.read().max_handlers_per_type
            )));
        }

        // Validate handler if enabled
        if self.config.read().enable_validation {
            self.validate_resource_handler(&handler)?;
        }

        // Register the handler
        self.resources.insert(name.clone(), Arc::new(handler));

        // Store metadata
        let metadata = HandlerMetadata {
            name: name.clone(),
            version: "1.0.0".to_string(),
            description: None,
            tags: vec!["resource".to_string()],
            created_at: chrono::Utc::now(),
            config: HashMap::new(),
            metrics_enabled: self.config.read().enable_metrics,
            rate_limit: None,
            allowed_roles: None,
        };
        self.metadata.insert(format!("resource:{name}"), metadata);

        tracing::info!("Registered resource handler: {}", name);
        Ok(())
    }

    /// Register a sampling handler
    pub fn register_sampling<S>(&self, name: impl Into<String>, handler: S) -> ServerResult<()>
    where
        S: SamplingHandler + 'static,
    {
        let name = name.into();

        // Check limits
        if self.sampling.len() >= self.config.read().max_handlers_per_type {
            return Err(ServerError::handler(format!(
                "Maximum number of sampling handlers ({}) exceeded",
                self.config.read().max_handlers_per_type
            )));
        }

        self.sampling.insert(name.clone(), Arc::new(handler));

        // Store metadata
        let metadata = HandlerMetadata {
            name: name.clone(),
            version: "1.0.0".to_string(),
            description: None,
            tags: vec!["sampling".to_string()],
            created_at: chrono::Utc::now(),
            config: HashMap::new(),
            metrics_enabled: self.config.read().enable_metrics,
            rate_limit: None,
            allowed_roles: None,
        };
        self.metadata.insert(format!("sampling:{name}"), metadata);

        tracing::info!("Registered sampling handler: {}", name);
        Ok(())
    }

    /// Register a logging handler
    pub fn register_logging<L>(&self, name: impl Into<String>, handler: L) -> ServerResult<()>
    where
        L: LoggingHandler + 'static,
    {
        let name = name.into();

        // Check limits
        if self.logging.len() >= self.config.read().max_handlers_per_type {
            return Err(ServerError::handler(format!(
                "Maximum number of logging handlers ({}) exceeded",
                self.config.read().max_handlers_per_type
            )));
        }

        self.logging.insert(name.clone(), Arc::new(handler));

        // Store metadata
        let metadata = HandlerMetadata {
            name: name.clone(),
            version: "1.0.0".to_string(),
            description: None,
            tags: vec!["logging".to_string()],
            created_at: chrono::Utc::now(),
            config: HashMap::new(),
            metrics_enabled: self.config.read().enable_metrics,
            rate_limit: None,
            allowed_roles: None,
        };
        self.metadata.insert(format!("logging:{name}"), metadata);

        tracing::info!("Registered logging handler: {}", name);
        Ok(())
    }

    /// Get a tool handler by name
    #[must_use]
    pub fn get_tool(&self, name: &str) -> Option<Arc<dyn ToolHandler>> {
        self.tools.get(name).map(|entry| Arc::clone(entry.value()))
    }

    /// Get a prompt handler by name
    #[must_use]
    pub fn get_prompt(&self, name: &str) -> Option<Arc<dyn PromptHandler>> {
        self.prompts
            .get(name)
            .map(|entry| Arc::clone(entry.value()))
    }

    /// Get a resource handler by name
    #[must_use]
    pub fn get_resource(&self, name: &str) -> Option<Arc<dyn ResourceHandler>> {
        self.resources
            .get(name)
            .map(|entry| Arc::clone(entry.value()))
    }

    /// Get a sampling handler by name
    #[must_use]
    pub fn get_sampling(&self, name: &str) -> Option<Arc<dyn SamplingHandler>> {
        self.sampling
            .get(name)
            .map(|entry| Arc::clone(entry.value()))
    }

    /// Get a logging handler by name
    #[must_use]
    pub fn get_logging(&self, name: &str) -> Option<Arc<dyn LoggingHandler>> {
        self.logging
            .get(name)
            .map(|entry| Arc::clone(entry.value()))
    }

    /// List all tool names
    #[must_use]
    pub fn list_tools(&self) -> Vec<String> {
        self.tools.iter().map(|entry| entry.key().clone()).collect()
    }

    /// List all prompt names
    #[must_use]
    pub fn list_prompts(&self) -> Vec<String> {
        self.prompts
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// List all resource names
    #[must_use]
    pub fn list_resources(&self) -> Vec<String> {
        self.resources
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// List all sampling names
    #[must_use]
    pub fn list_sampling(&self) -> Vec<String> {
        self.sampling
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// List all logging names
    #[must_use]
    pub fn list_logging(&self) -> Vec<String> {
        self.logging
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Get all tool definitions
    #[must_use]
    pub fn get_tool_definitions(&self) -> Vec<Tool> {
        self.tools
            .iter()
            .map(|entry| entry.value().tool_definition())
            .collect()
    }

    /// Get all prompt definitions
    #[must_use]
    pub fn get_prompt_definitions(&self) -> Vec<Prompt> {
        self.prompts
            .iter()
            .map(|entry| entry.value().prompt_definition())
            .collect()
    }

    /// Get all resource definitions
    #[must_use]
    pub fn get_resource_definitions(&self) -> Vec<Resource> {
        self.resources
            .iter()
            .map(|entry| entry.value().resource_definition())
            .collect()
    }

    /// Add a root to the registry
    pub fn add_root(&self, root: Root) {
        self.roots.write().push(root);
    }

    /// Set multiple roots at once
    pub fn set_roots(&self, roots: Vec<Root>) {
        *self.roots.write() = roots;
    }

    /// Get all registered roots
    #[must_use]
    pub fn get_roots(&self) -> Vec<Root> {
        self.roots.read().clone()
    }

    /// Clear all roots
    pub fn clear_roots(&self) {
        self.roots.write().clear();
    }

    /// Unregister a tool handler
    pub fn unregister_tool(&self, name: &str) -> bool {
        let removed = self.tools.remove(name).is_some();
        if removed {
            self.metadata.remove(&format!("tool:{name}"));
            tracing::info!("Unregistered tool handler: {}", name);
        }
        removed
    }

    /// Unregister a prompt handler
    pub fn unregister_prompt(&self, name: &str) -> bool {
        let removed = self.prompts.remove(name).is_some();
        if removed {
            self.metadata.remove(&format!("prompt:{name}"));
            tracing::info!("Unregistered prompt handler: {}", name);
        }
        removed
    }

    /// Unregister a resource handler
    pub fn unregister_resource(&self, name: &str) -> bool {
        let removed = self.resources.remove(name).is_some();
        if removed {
            self.metadata.remove(&format!("resource:{name}"));
            tracing::info!("Unregistered resource handler: {}", name);
        }
        removed
    }

    /// Clear all handlers
    pub fn clear(&self) {
        self.tools.clear();
        self.prompts.clear();
        self.resources.clear();
        self.sampling.clear();
        self.logging.clear();
        self.metadata.clear();
        tracing::info!("Cleared all handlers from registry");
    }

    /// Get registry statistics
    #[must_use]
    pub fn stats(&self) -> RegistryStats {
        RegistryStats {
            tool_count: self.tools.len(),
            prompt_count: self.prompts.len(),
            resource_count: self.resources.len(),
            sampling_count: self.sampling.len(),
            logging_count: self.logging.len(),
            total_count: self.tools.len()
                + self.prompts.len()
                + self.resources.len()
                + self.sampling.len()
                + self.logging.len(),
        }
    }

    /// Get handler metadata
    #[must_use]
    pub fn get_metadata(&self, key: &str) -> Option<HandlerMetadata> {
        self.metadata.get(key).map(|entry| entry.value().clone())
    }

    /// Update registry configuration
    pub fn update_config<F>(&self, f: F)
    where
        F: FnOnce(&mut RegistryConfig),
    {
        let mut config = self.config.write();
        f(&mut config);
    }

    // Private validation methods

    fn validate_tool_handler(&self, handler: &dyn ToolHandler) -> ServerResult<()> {
        let tool_def = handler.tool_definition();

        if tool_def.name.is_empty() {
            return Err(ServerError::handler("Tool name cannot be empty"));
        }

        if tool_def.name.len() > 100 {
            return Err(ServerError::handler(
                "Tool name too long (max 100 characters)",
            ));
        }

        // Check for duplicate names
        if self.tools.contains_key(&tool_def.name) {
            return Err(ServerError::handler(format!(
                "Tool with name '{}' already exists",
                tool_def.name
            )));
        }

        Ok(())
    }

    fn validate_prompt_handler(&self, handler: &dyn PromptHandler) -> ServerResult<()> {
        let prompt_def = handler.prompt_definition();

        if prompt_def.name.is_empty() {
            return Err(ServerError::handler("Prompt name cannot be empty"));
        }

        if prompt_def.name.len() > 100 {
            return Err(ServerError::handler(
                "Prompt name too long (max 100 characters)",
            ));
        }

        // Check for duplicate names
        if self.prompts.contains_key(&prompt_def.name) {
            return Err(ServerError::handler(format!(
                "Prompt with name '{}' already exists",
                prompt_def.name
            )));
        }

        Ok(())
    }

    fn validate_resource_handler(&self, handler: &dyn ResourceHandler) -> ServerResult<()> {
        let resource_def = handler.resource_definition();

        if resource_def.uri.is_empty() {
            return Err(ServerError::handler("Resource URI cannot be empty"));
        }

        if resource_def.name.is_empty() {
            return Err(ServerError::handler("Resource name cannot be empty"));
        }

        // Check for duplicate URIs
        for entry in &self.resources {
            if entry.value().resource_definition().uri == resource_def.uri {
                return Err(ServerError::handler(format!(
                    "Resource with URI '{}' already exists",
                    resource_def.uri
                )));
            }
        }

        Ok(())
    }
}

impl Default for HandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Registry statistics
#[derive(Debug, Clone)]
pub struct RegistryStats {
    /// Number of tool handlers
    pub tool_count: usize,
    /// Number of prompt handlers
    pub prompt_count: usize,
    /// Number of resource handlers
    pub resource_count: usize,
    /// Number of sampling handlers
    pub sampling_count: usize,
    /// Number of logging handlers
    pub logging_count: usize,
    /// Total number of handlers
    pub total_count: usize,
}

/// Registry builder for configuring the registry
#[derive(Debug)]
pub struct RegistryBuilder {
    config: RegistryConfig,
}

impl RegistryBuilder {
    /// Create a new registry builder
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: RegistryConfig::default(),
        }
    }

    /// Set maximum handlers per type
    #[must_use]
    pub const fn max_handlers_per_type(mut self, max: usize) -> Self {
        self.config.max_handlers_per_type = max;
        self
    }

    /// Enable or disable metrics
    #[must_use]
    pub const fn enable_metrics(mut self, enable: bool) -> Self {
        self.config.enable_metrics = enable;
        self
    }

    /// Enable or disable validation
    #[must_use]
    pub const fn enable_validation(mut self, enable: bool) -> Self {
        self.config.enable_validation = enable;
        self
    }

    /// Set handler timeout
    #[must_use]
    pub const fn handler_timeout_ms(mut self, timeout: u64) -> Self {
        self.config.handler_timeout_ms = timeout;
        self
    }

    /// Enable or disable hot reload
    #[must_use]
    pub const fn enable_hot_reload(mut self, enable: bool) -> Self {
        self.config.enable_hot_reload = enable;
        self
    }

    /// Build the registry
    #[must_use]
    pub fn build(self) -> HandlerRegistry {
        HandlerRegistry::with_config(self.config)
    }
}

impl Default for RegistryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Main registry interface (alias for `HandlerRegistry`)
pub type Registry = HandlerRegistry;
