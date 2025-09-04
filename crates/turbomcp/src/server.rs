//! `TurboMCP` server implementation

//use async_trait::async_trait;
//use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// Handler information for registration
#[derive(Debug, Clone)]
pub struct HandlerInfo {
    /// Handler name
    pub name: String,
    /// Handler type
    pub handler_type: HandlerType,
    /// Handler description  
    pub description: Option<String>,
    /// Handler metadata
    pub metadata: serde_json::Value,
}

/// Handler type enumeration
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandlerType {
    /// Tool handler
    Tool,
    /// Prompt handler
    Prompt,
    /// Resource handler
    Resource,
}

/// Global handler registry
static HANDLER_REGISTRY: Mutex<Vec<HandlerInfo>> = Mutex::new(Vec::new());

/// Register a handler globally
pub fn register_handler(info: HandlerInfo) {
    if let Ok(mut registry) = HANDLER_REGISTRY.lock() {
        registry.push(info);
    } else {
        // Mutex is poisoned - this is a critical error but we shouldn't panic
        tracing::error!("Handler registry mutex poisoned - unable to register handler");
    }
}

/// Get all registered handlers
pub fn get_registered_handlers() -> Vec<HandlerInfo> {
    HANDLER_REGISTRY
        .lock()
        .map(|registry| registry.clone())
        .unwrap_or_else(|_| {
            tracing::error!("Handler registry mutex poisoned - returning empty registry");
            Vec::new()
        })
}
