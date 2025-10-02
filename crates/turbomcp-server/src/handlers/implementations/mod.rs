//! Concrete handler implementations
//!
//! This module contains concrete implementations of the handler traits,
//! including function-based handlers and other implementations.

pub mod function_tool;

// Re-export implementations for backwards compatibility
pub use function_tool::FunctionToolHandler;
