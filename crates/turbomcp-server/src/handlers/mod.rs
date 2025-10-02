//! Handler traits and implementations for MCP operations
//!
//! This module contains the decomposed handler system with focused modules:
//!
//! - `traits`: All handler trait definitions
//! - `composite`: Composite handler pattern for multi-type handlers
//! - `wrapper`: Handler wrapper infrastructure with metadata
//! - `implementations`: Concrete handler implementations
//! - `utils`: Utility functions for creating handlers from closures

use std::sync::Arc;

// Core modules
pub mod composite;
pub mod implementations;
pub mod traits;
pub mod utils;
pub mod wrapper;

// Re-export main types for backwards compatibility
pub use composite::CompositeHandler;
pub use implementations::FunctionToolHandler;
pub use traits::*;
pub use utils::{FunctionPromptHandler, FunctionResourceHandler};
pub use wrapper::{HandlerMetadata, HandlerWrapper};

/// Type alias for existence check functions to reduce complexity
pub type ExistenceCheckFn =
    Arc<dyn Fn(&str) -> futures::future::BoxFuture<'static, bool> + Send + Sync>;
