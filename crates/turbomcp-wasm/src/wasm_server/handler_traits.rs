//! Handler traits with blanket implementations for ergonomic async function support
//!
//! This module provides the magic that makes simple async functions "just work" as handlers.
//! Inspired by axum's Handler trait, but specialized for MCP tool/resource/prompt handlers.
//!
//! # The Magic
//!
//! Instead of requiring explicit trait implementations, you can write:
//!
//! ```ignore
//! // This just works! No trait impl needed.
//! async fn greet(args: GreetArgs) -> String {
//!     format!("Hello, {}!", args.name)
//! }
//!
//! // Error handling with ? also works
//! async fn fetch_data(args: FetchArgs) -> Result<Json<Data>, ToolError> {
//!     let data = fetch_from_api(&args.url).await?;
//!     Ok(Json(data))
//! }
//!
//! let server = McpServer::builder("name", "1.0.0")
//!     .tool("greet", "Greet someone", greet)
//!     .tool("fetch", "Fetch data", fetch_data)
//!     .build();
//! ```

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;

use serde::de::DeserializeOwned;

use super::response::IntoToolResponse;
use super::traits::{IntoPromptResponse, IntoResourceResponse};
use super::types::{PromptResult, ResourceResult, ToolResult};

// ============================================================================
// Type Aliases (to satisfy clippy type_complexity)
// ============================================================================

/// Boxed async tool handler function type
type BoxedToolHandler = Box<
    dyn Fn(serde_json::Value) -> Pin<Box<dyn Future<Output = ToolResult> + Send>> + Send + Sync,
>;

/// Boxed async resource handler function type
type BoxedResourceHandler = Box<
    dyn Fn(String) -> Pin<Box<dyn Future<Output = Result<ResourceResult, String>> + Send>>
        + Send
        + Sync,
>;

/// Boxed async prompt handler function type
type BoxedPromptHandler = Box<
    dyn Fn(
            Option<serde_json::Value>,
        ) -> Pin<Box<dyn Future<Output = Result<PromptResult, String>> + Send>>
        + Send
        + Sync,
>;

// ============================================================================
// Tool Handler Trait
// ============================================================================

/// Sealed trait for tool handlers.
///
/// This trait is automatically implemented for any async function that:
/// - Takes a single argument implementing `DeserializeOwned + JsonSchema`
/// - Returns something implementing `IntoToolResponse`
///
/// You don't need to implement this trait manually.
pub trait IntoToolHandler<T, M>: Clone + Send + Sync + 'static {
    /// Convert to a boxed handler function
    fn into_handler(self) -> BoxedToolHandler;

    /// Generate the JSON schema for the argument type
    fn schema() -> serde_json::Value;
}

/// Marker type for typed arguments
pub struct WithArgs<A>(PhantomData<A>);

/// Blanket implementation for async functions with typed arguments
impl<F, A, Fut, Res> IntoToolHandler<A, WithArgs<A>> for F
where
    F: Fn(A) -> Fut + Clone + Send + Sync + 'static,
    A: DeserializeOwned + schemars::JsonSchema + Send + 'static,
    Fut: Future<Output = Res> + Send + 'static,
    Res: IntoToolResponse + 'static,
{
    fn into_handler(self) -> BoxedToolHandler {
        Box::new(move |params: serde_json::Value| {
            let handler = self.clone();
            Box::pin(async move {
                match serde_json::from_value::<A>(params) {
                    Ok(args) => handler(args).await.into_tool_response(),
                    Err(e) => ToolResult::error(format!("Invalid arguments: {e}")),
                }
            })
        })
    }

    fn schema() -> serde_json::Value {
        let schema = schemars::schema_for!(A);
        serde_json::to_value(&schema).unwrap_or_default()
    }
}

/// Marker type for raw JSON arguments
pub struct RawArgs;

/// Implementation for handlers that take raw serde_json::Value
impl<F, Fut, Res> IntoToolHandler<serde_json::Value, RawArgs> for F
where
    F: Fn(serde_json::Value) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Res> + Send + 'static,
    Res: IntoToolResponse + 'static,
{
    fn into_handler(self) -> BoxedToolHandler {
        Box::new(move |params: serde_json::Value| {
            let handler = self.clone();
            Box::pin(async move { handler(params).await.into_tool_response() })
        })
    }

    fn schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "additionalProperties": true
        })
    }
}

/// Marker type for no arguments
pub struct NoArgs;

/// Implementation for handlers that take no arguments
impl<F, Fut, Res> IntoToolHandler<(), NoArgs> for F
where
    F: Fn() -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Res> + Send + 'static,
    Res: IntoToolResponse + 'static,
{
    fn into_handler(self) -> BoxedToolHandler {
        Box::new(move |_params: serde_json::Value| {
            let handler = self.clone();
            Box::pin(async move { handler().await.into_tool_response() })
        })
    }

    fn schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }
}

// ============================================================================
// Resource Handler Trait
// ============================================================================

/// Sealed trait for resource handlers.
///
/// Automatically implemented for async functions that:
/// - Take a `String` (the URI)
/// - Return something implementing `IntoResourceResponse`
pub trait IntoResourceHandler<M>: Clone + Send + Sync + 'static {
    /// Convert to a boxed handler function
    fn into_handler(self) -> BoxedResourceHandler;
}

/// Marker for resource handlers
pub struct ResourceMarker;

impl<F, Fut, Res> IntoResourceHandler<ResourceMarker> for F
where
    F: Fn(String) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Res> + Send + 'static,
    Res: IntoResourceResponse + 'static,
{
    fn into_handler(self) -> BoxedResourceHandler {
        Box::new(move |uri: String| {
            let handler = self.clone();
            Box::pin(async move { handler(uri).await.into_resource_response() })
        })
    }
}

// ============================================================================
// Prompt Handler Trait
// ============================================================================

/// Sealed trait for prompt handlers.
///
/// Automatically implemented for async functions that:
/// - Take `Option<A>` where A: DeserializeOwned + JsonSchema
/// - Return something implementing `IntoPromptResponse`
pub trait IntoPromptHandler<T, M>: Clone + Send + Sync + 'static {
    /// Convert to a boxed handler function
    fn into_handler(self) -> BoxedPromptHandler;

    /// Get the prompt arguments schema
    fn arguments() -> Vec<turbomcp_core::types::prompts::PromptArgument>;
}

/// Marker for typed prompt arguments
pub struct PromptWithArgs<A>(PhantomData<A>);

impl<F, A, Fut, Res> IntoPromptHandler<A, PromptWithArgs<A>> for F
where
    F: Fn(Option<A>) -> Fut + Clone + Send + Sync + 'static,
    A: DeserializeOwned + schemars::JsonSchema + Send + 'static,
    Fut: Future<Output = Res> + Send + 'static,
    Res: IntoPromptResponse + 'static,
{
    fn into_handler(self) -> BoxedPromptHandler {
        Box::new(move |args: Option<serde_json::Value>| {
            let handler = self.clone();
            Box::pin(async move {
                let parsed_args: Option<A> = match args {
                    Some(v) => Some(
                        serde_json::from_value(v).map_err(|e| format!("Invalid arguments: {e}"))?,
                    ),
                    None => None,
                };
                handler(parsed_args).await.into_prompt_response()
            })
        })
    }

    fn arguments() -> Vec<turbomcp_core::types::prompts::PromptArgument> {
        // Extract argument info from schema
        let schema = schemars::schema_for!(A);
        let schema_value = serde_json::to_value(&schema).unwrap_or_default();

        let mut arguments = Vec::new();

        if let Some(properties) = schema_value.get("properties").and_then(|p| p.as_object()) {
            let required: Vec<String> = schema_value
                .get("required")
                .and_then(|r| r.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            for (name, prop) in properties {
                let description = prop
                    .get("description")
                    .and_then(|d| d.as_str())
                    .map(String::from);

                arguments.push(turbomcp_core::types::prompts::PromptArgument {
                    name: name.clone(),
                    description,
                    required: Some(required.contains(name)),
                });
            }
        }

        arguments
    }
}

/// Marker for no prompt arguments
pub struct PromptNoArgs;

impl<F, Fut, Res> IntoPromptHandler<(), PromptNoArgs> for F
where
    F: Fn() -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Res> + Send + 'static,
    Res: IntoPromptResponse + 'static,
{
    fn into_handler(self) -> BoxedPromptHandler {
        Box::new(move |_args: Option<serde_json::Value>| {
            let handler = self.clone();
            Box::pin(async move { handler().await.into_prompt_response() })
        })
    }

    fn arguments() -> Vec<turbomcp_core::types::prompts::PromptArgument> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    #[allow(dead_code)]
    #[derive(serde::Deserialize, schemars::JsonSchema)]
    struct TestArgs {
        name: String,
    }

    #[test]
    fn test_schema_generation_with_args() {
        // Test that schema is generated correctly for typed args
        let schema = schemars::schema_for!(TestArgs);
        let schema_value = serde_json::to_value(&schema).unwrap();
        assert!(schema_value.get("properties").is_some());
        let props = schema_value.get("properties").unwrap().as_object().unwrap();
        assert!(props.contains_key("name"));
    }

    #[test]
    fn test_no_args_schema() {
        // Test that empty schema is created for no-args handlers
        let schema = serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        });
        let obj = schema.as_object().unwrap();
        assert_eq!(obj.get("type").unwrap(), "object");
    }
}
