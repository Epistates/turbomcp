//! Code generation layer
//!
//! This module provides code generation capabilities for creating optimized
//! Rust proxies from MCP server specifications.

#[cfg(feature = "codegen")]
pub mod template_engine;

#[cfg(feature = "codegen")]
pub mod context;

#[cfg(feature = "codegen")]
pub mod generator;

#[cfg(feature = "codegen")]
pub mod type_generator;

#[cfg(feature = "codegen")]
pub mod sanitize;

// Re-export main types
#[cfg(feature = "codegen")]
pub use generator::{BackendType, FrontendType, GenConfig, GeneratedProject, RustCodeGenerator};

#[cfg(feature = "codegen")]
pub use context::{
    CargoContext, FieldDefinition, MainContext, PromptDefinition, ProxyContext, ResourceDefinition,
    ToolDefinition, TypeDefinition, TypesContext,
};

#[cfg(feature = "codegen")]
pub use template_engine::TemplateEngine;

#[cfg(feature = "codegen")]
pub use type_generator::TypeGenerator;

#[cfg(feature = "codegen")]
pub use sanitize::{
    is_rust_keyword, sanitize_identifier, sanitize_string_literal, sanitize_type, sanitize_uri,
};

// Placeholder for when codegen feature is not enabled
#[cfg(not(feature = "codegen"))]
pub struct RustCodeGenerator {
    _private: (),
}

#[cfg(not(feature = "codegen"))]
impl RustCodeGenerator {
    pub fn new() -> Self {
        panic!("Code generation requires the 'codegen' feature to be enabled");
    }
}

#[cfg(not(feature = "codegen"))]
impl Default for RustCodeGenerator {
    fn default() -> Self {
        Self::new()
    }
}
