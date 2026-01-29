//! Trait-based handler registration for MCP servers
//!
//! This module provides traits that allow tool, resource, and prompt handlers
//! to be implemented as structs with trait implementations, offering an
//! alternative to closure-based registration.
//!
//! # Example
//!
//! ```ignore
//! use turbomcp_wasm::wasm_server::*;
//! use async_trait::async_trait;
//!
//! struct Calculator;
//!
//! #[async_trait(?Send)]
//! impl ToolHandler for Calculator {
//!     type Args = CalculatorArgs;
//!
//!     fn name(&self) -> &str { "calculator" }
//!     fn description(&self) -> &str { "Perform calculations" }
//!
//!     async fn call(&self, args: Self::Args) -> impl IntoToolResponse {
//!         match args.operation.as_str() {
//!             "add" => Json(args.a + args.b),
//!             "sub" => Json(args.a - args.b),
//!             _ => Err(ToolError::new("Unknown operation"))
//!         }
//!     }
//! }
//!
//! #[derive(Deserialize, JsonSchema)]
//! struct CalculatorArgs {
//!     operation: String,
//!     a: i64,
//!     b: i64,
//! }
//! ```

use std::future::Future;

use serde::de::DeserializeOwned;
use turbomcp_core::{MaybeSend, MaybeSync};

use super::response::{IntoToolResponse, ToolError};
use super::types::{PromptResult, ResourceResult};

/// Trait for implementing tool handlers as structs.
///
/// This provides an alternative to closure-based tool registration,
/// useful for more complex tools or when you want to organize code
/// into separate modules.
///
/// # Example
///
/// ```ignore
/// struct MyTool {
///     api_key: String,
/// }
///
/// impl ToolHandlerFn for MyTool {
///     type Args = MyToolArgs;
///     type Output = Result<String, ToolError>;
///
///     fn name(&self) -> &str { "my_tool" }
///     fn description(&self) -> &str { "Does something useful" }
///
///     async fn call(&self, args: Self::Args) -> Self::Output {
///         // Use self.api_key here
///         Ok(format!("Called with: {}", args.input))
///     }
/// }
/// ```
pub trait ToolHandlerFn: MaybeSend + MaybeSync + 'static {
    /// The argument type for this tool (must implement Deserialize and JsonSchema)
    type Args: DeserializeOwned + schemars::JsonSchema + 'static;

    /// The output type (must implement IntoToolResponse)
    type Output: IntoToolResponse;

    /// The future type returned by call
    type Future: Future<Output = Self::Output> + MaybeSend + 'static;

    /// Returns the name of the tool
    fn name(&self) -> &str;

    /// Returns the description of the tool
    fn description(&self) -> &str;

    /// Execute the tool with the given arguments
    fn call(&self, args: Self::Args) -> Self::Future;
}

/// Trait for implementing resource handlers as structs.
///
/// # Example
///
/// ```ignore
/// struct ConfigResource {
///     config_path: PathBuf,
/// }
///
/// impl ResourceHandlerFn for ConfigResource {
///     type Output = Result<ResourceResult, ToolError>;
///
///     fn uri(&self) -> &str { "config://app" }
///     fn name(&self) -> &str { "Application Config" }
///     fn description(&self) -> &str { "Current application configuration" }
///
///     async fn read(&self, uri: String) -> Self::Output {
///         let content = std::fs::read_to_string(&self.config_path)?;
///         Ok(ResourceResult::text(uri, content))
///     }
/// }
/// ```
pub trait ResourceHandlerFn: MaybeSend + MaybeSync + 'static {
    /// The output type (typically Result<ResourceResult, ToolError>)
    type Output: IntoResourceResponse;

    /// The future type returned by read
    type Future: Future<Output = Self::Output> + MaybeSend + 'static;

    /// Returns the URI of the resource
    fn uri(&self) -> &str;

    /// Returns the name of the resource
    fn name(&self) -> &str;

    /// Returns the description of the resource
    fn description(&self) -> &str;

    /// Read the resource content
    fn read(&self, uri: String) -> Self::Future;
}

/// Trait for implementing prompt handlers as structs.
///
/// # Example
///
/// ```ignore
/// struct GreetingPrompt;
///
/// impl PromptHandlerFn for GreetingPrompt {
///     type Args = GreetingArgs;
///     type Output = Result<PromptResult, ToolError>;
///
///     fn name(&self) -> &str { "greeting" }
///     fn description(&self) -> &str { "Generate a greeting" }
///
///     async fn get(&self, args: Option<Self::Args>) -> Self::Output {
///         let name = args.map(|a| a.name).unwrap_or_else(|| "World".into());
///         Ok(PromptResult::user(format!("Hello, {}!", name)))
///     }
/// }
/// ```
pub trait PromptHandlerFn: MaybeSend + MaybeSync + 'static {
    /// The argument type for this prompt (must implement Deserialize and JsonSchema)
    type Args: DeserializeOwned + schemars::JsonSchema + 'static;

    /// The output type (typically Result<PromptResult, ToolError>)
    type Output: IntoPromptResponse;

    /// The future type returned by get
    type Future: Future<Output = Self::Output> + MaybeSend + 'static;

    /// Returns the name of the prompt
    fn name(&self) -> &str;

    /// Returns the description of the prompt
    fn description(&self) -> &str;

    /// Get the prompt with the given arguments
    fn get(&self, args: Option<Self::Args>) -> Self::Future;
}

/// Trait for types that can be converted into a resource response.
pub trait IntoResourceResponse {
    /// Convert this type into a resource result
    fn into_resource_response(self) -> Result<ResourceResult, String>;
}

impl IntoResourceResponse for ResourceResult {
    #[inline]
    fn into_resource_response(self) -> Result<ResourceResult, String> {
        Ok(self)
    }
}

impl<E: std::fmt::Display> IntoResourceResponse for Result<ResourceResult, E> {
    fn into_resource_response(self) -> Result<ResourceResult, String> {
        self.map_err(|e| e.to_string())
    }
}

/// Trait for types that can be converted into a prompt response.
pub trait IntoPromptResponse {
    /// Convert this type into a prompt result
    fn into_prompt_response(self) -> Result<PromptResult, String>;
}

impl IntoPromptResponse for PromptResult {
    #[inline]
    fn into_prompt_response(self) -> Result<PromptResult, String> {
        Ok(self)
    }
}

impl<E: std::fmt::Display> IntoPromptResponse for Result<PromptResult, E> {
    fn into_prompt_response(self) -> Result<PromptResult, String> {
        self.map_err(|e| e.to_string())
    }
}

// ============================================================================
// Extension trait for ergonomic Result handling in resources/prompts
// ============================================================================

/// Extension trait providing `.map_tool_err()` for easy error context.
pub trait ResultExt<T, E> {
    /// Map the error to a ToolError with additional context
    fn map_tool_err(self, context: &str) -> Result<T, ToolError>;
}

impl<T, E: std::fmt::Display> ResultExt<T, E> for Result<T, E> {
    fn map_tool_err(self, context: &str) -> Result<T, ToolError> {
        self.map_err(|e| ToolError::new(format!("{}: {}", context, e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_result_ext_map_tool_err() {
        let result: Result<(), &str> = Err("original error");
        let mapped = result.map_tool_err("context");
        assert!(mapped.is_err());
        let err = mapped.unwrap_err();
        assert!(err.to_string().contains("context"));
        assert!(err.to_string().contains("original error"));
    }

    #[test]
    fn test_into_resource_response_ok() {
        let result = ResourceResult::text("uri", "content");
        let response = result.into_resource_response();
        assert!(response.is_ok());
    }

    #[test]
    fn test_into_resource_response_result_ok() {
        let result: Result<ResourceResult, String> = Ok(ResourceResult::text("uri", "content"));
        let response = result.into_resource_response();
        assert!(response.is_ok());
    }

    #[test]
    fn test_into_resource_response_result_err() {
        let result: Result<ResourceResult, String> = Err("failed".into());
        let response = result.into_resource_response();
        assert!(response.is_err());
        assert_eq!(response.unwrap_err(), "failed");
    }

    #[test]
    fn test_into_prompt_response_ok() {
        let result = PromptResult::user("hello");
        let response = result.into_prompt_response();
        assert!(response.is_ok());
    }

    #[test]
    fn test_into_prompt_response_result_err() {
        let result: Result<PromptResult, ToolError> = Err(ToolError::new("failed"));
        let response = result.into_prompt_response();
        assert!(response.is_err());
    }
}
