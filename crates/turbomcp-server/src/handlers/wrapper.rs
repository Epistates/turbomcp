//! Handler wrapper infrastructure for metadata and additional functionality

use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Handler wrapper that provides additional functionality
pub struct HandlerWrapper<T> {
    /// The wrapped handler
    handler: Arc<T>,
    /// Handler metadata
    metadata: HandlerMetadata,
}

impl<T> std::fmt::Debug for HandlerWrapper<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HandlerWrapper")
            .field("metadata", &self.metadata)
            .finish()
    }
}

/// Metadata associated with a handler
#[derive(Debug, Clone)]
pub struct HandlerMetadata {
    /// Handler name
    pub name: String,
    /// Handler version
    pub version: String,
    /// Handler description
    pub description: Option<String>,
    /// Handler tags
    pub tags: Vec<String>,
    /// Handler creation time
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Handler configuration
    pub config: HashMap<String, Value>,
    /// Handler metrics enabled
    pub metrics_enabled: bool,
    /// Handler rate limit (requests per second)
    pub rate_limit: Option<u32>,
    /// Allowed roles for authorization (if None or empty => allow all)
    pub allowed_roles: Option<Vec<String>>,
}

impl Default for HandlerMetadata {
    fn default() -> Self {
        Self {
            name: "unnamed".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            tags: Vec::new(),
            created_at: chrono::Utc::now(),
            config: HashMap::new(),
            metrics_enabled: true,
            rate_limit: None,
            allowed_roles: None,
        }
    }
}

impl<T> HandlerWrapper<T> {
    /// Create a new handler wrapper
    pub fn new(handler: T) -> Self {
        Self {
            handler: Arc::new(handler),
            metadata: HandlerMetadata::default(),
        }
    }

    /// Create a wrapper with metadata
    pub fn with_metadata(handler: T, metadata: HandlerMetadata) -> Self {
        Self {
            handler: Arc::new(handler),
            metadata,
        }
    }

    /// Get handler reference
    #[must_use]
    pub const fn handler(&self) -> &Arc<T> {
        &self.handler
    }

    /// Get handler metadata
    #[must_use]
    pub const fn metadata(&self) -> &HandlerMetadata {
        &self.metadata
    }

    /// Update handler metadata
    pub fn update_metadata<F>(&mut self, f: F)
    where
        F: FnOnce(&mut HandlerMetadata),
    {
        f(&mut self.metadata);
    }
}

impl<T: Clone> Clone for HandlerWrapper<T> {
    fn clone(&self) -> Self {
        Self {
            handler: Arc::clone(&self.handler),
            metadata: self.metadata.clone(),
        }
    }
}
