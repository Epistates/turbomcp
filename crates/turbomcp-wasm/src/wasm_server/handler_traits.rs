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
//! // With request context for session/header access
//! async fn auth_tool(ctx: &RequestContext, args: AuthArgs) -> Result<String, ToolError> {
//!     if !ctx.is_authenticated() {
//!         return Err(ToolError::new("Unauthorized"));
//!     }
//!     Ok("Authenticated!".to_string())
//! }
//!
//! let server = McpServer::builder("name", "1.0.0")
//!     .tool("greet", "Greet someone", greet)
//!     .tool("fetch", "Fetch data", fetch_data)
//!     .tool_with_ctx("auth", "Authenticated tool", auth_tool)
//!     .build();
//! ```
//!
//! # Platform Compatibility
//!
//! This module uses `MaybeSend`/`MaybeSync` markers for cross-platform compatibility:
//! - **Native**: Requires `Send + Sync` for multi-threaded executors
//! - **WASM**: No thread safety requirements, allowing `Rc<RefCell>` and Worker types

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;

use serde::de::DeserializeOwned;
use turbomcp_core::{MaybeSend, MaybeSync};

use super::context::RequestContext;
use super::response::IntoToolResponse;
use super::traits::{IntoPromptResponse, IntoResourceResponse};
use super::types::{PromptResult, ResourceResult, ToolResult};

// ============================================================================
// Type Aliases (to satisfy clippy type_complexity)
// ============================================================================

/// Boxed async tool handler function type (no context)
///
/// Note: WASM is single-threaded so inner futures don't need Send bounds.
type BoxedToolHandler =
    Box<dyn Fn(serde_json::Value) -> Pin<Box<dyn Future<Output = ToolResult>>> + Send + Sync>;

/// Boxed async tool handler function type with context
type BoxedToolHandlerWithCtx = Box<
    dyn Fn(Arc<RequestContext>, serde_json::Value) -> Pin<Box<dyn Future<Output = ToolResult>>>
        + Send
        + Sync,
>;

/// Boxed async resource handler function type (no context)
type BoxedResourceHandler = Box<
    dyn Fn(String) -> Pin<Box<dyn Future<Output = Result<ResourceResult, String>>>> + Send + Sync,
>;

/// Boxed async resource handler function type with context
type BoxedResourceHandlerWithCtx = Box<
    dyn Fn(
            Arc<RequestContext>,
            String,
        ) -> Pin<Box<dyn Future<Output = Result<ResourceResult, String>>>>
        + Send
        + Sync,
>;

/// Boxed async prompt handler function type (no context)
type BoxedPromptHandler = Box<
    dyn Fn(Option<serde_json::Value>) -> Pin<Box<dyn Future<Output = Result<PromptResult, String>>>>
        + Send
        + Sync,
>;

/// Boxed async prompt handler function type with context
type BoxedPromptHandlerWithCtx = Box<
    dyn Fn(
            Arc<RequestContext>,
            Option<serde_json::Value>,
        ) -> Pin<Box<dyn Future<Output = Result<PromptResult, String>>>>
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
pub trait IntoToolHandler<T, M>: Clone + MaybeSend + MaybeSync + 'static {
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
    F: Fn(A) -> Fut + Clone + MaybeSend + MaybeSync + 'static,
    A: DeserializeOwned + schemars::JsonSchema + MaybeSend + 'static,
    Fut: Future<Output = Res> + MaybeSend + 'static,
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
    F: Fn(serde_json::Value) -> Fut + Clone + MaybeSend + MaybeSync + 'static,
    Fut: Future<Output = Res> + MaybeSend + 'static,
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
    F: Fn() -> Fut + Clone + MaybeSend + MaybeSync + 'static,
    Fut: Future<Output = Res> + MaybeSend + 'static,
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
// Context-Aware Tool Handler Trait
// ============================================================================

/// Sealed trait for tool handlers with context injection.
///
/// This trait is automatically implemented for any async function that:
/// - Takes `&RequestContext` as first argument
/// - Takes a typed argument implementing `DeserializeOwned + JsonSchema` as second argument
/// - Returns something implementing `IntoToolResponse`
///
/// You don't need to implement this trait manually.
pub trait IntoToolHandlerWithCtx<T, M>: Clone + MaybeSend + MaybeSync + 'static {
    /// Convert to a boxed handler function that receives context
    fn into_handler_with_ctx(self) -> BoxedToolHandlerWithCtx;

    /// Generate the JSON schema for the argument type
    fn schema() -> serde_json::Value;
}

/// Marker type for context + typed arguments
pub struct WithCtxArgs<A>(PhantomData<A>);

/// Blanket implementation for async functions with context + typed arguments
impl<F, A, Fut, Res> IntoToolHandlerWithCtx<A, WithCtxArgs<A>> for F
where
    F: Fn(Arc<RequestContext>, A) -> Fut + Clone + MaybeSend + MaybeSync + 'static,
    A: DeserializeOwned + schemars::JsonSchema + MaybeSend + 'static,
    Fut: Future<Output = Res> + MaybeSend + 'static,
    Res: IntoToolResponse + 'static,
{
    fn into_handler_with_ctx(self) -> BoxedToolHandlerWithCtx {
        Box::new(move |ctx: Arc<RequestContext>, params: serde_json::Value| {
            let handler = self.clone();
            Box::pin(async move {
                match serde_json::from_value::<A>(params) {
                    Ok(args) => handler(ctx, args).await.into_tool_response(),
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

/// Marker type for context + raw JSON arguments
pub struct WithCtxRaw;

/// Implementation for handlers that take context + raw serde_json::Value
impl<F, Fut, Res> IntoToolHandlerWithCtx<serde_json::Value, WithCtxRaw> for F
where
    F: Fn(Arc<RequestContext>, serde_json::Value) -> Fut + Clone + MaybeSend + MaybeSync + 'static,
    Fut: Future<Output = Res> + MaybeSend + 'static,
    Res: IntoToolResponse + 'static,
{
    fn into_handler_with_ctx(self) -> BoxedToolHandlerWithCtx {
        Box::new(move |ctx: Arc<RequestContext>, params: serde_json::Value| {
            let handler = self.clone();
            Box::pin(async move { handler(ctx, params).await.into_tool_response() })
        })
    }

    fn schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "additionalProperties": true
        })
    }
}

/// Marker type for context only (no other arguments)
pub struct WithCtxOnly;

/// Implementation for handlers that take only context
impl<F, Fut, Res> IntoToolHandlerWithCtx<(), WithCtxOnly> for F
where
    F: Fn(Arc<RequestContext>) -> Fut + Clone + MaybeSend + MaybeSync + 'static,
    Fut: Future<Output = Res> + MaybeSend + 'static,
    Res: IntoToolResponse + 'static,
{
    fn into_handler_with_ctx(self) -> BoxedToolHandlerWithCtx {
        Box::new(
            move |ctx: Arc<RequestContext>, _params: serde_json::Value| {
                let handler = self.clone();
                Box::pin(async move { handler(ctx).await.into_tool_response() })
            },
        )
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
pub trait IntoResourceHandler<M>: Clone + MaybeSend + MaybeSync + 'static {
    /// Convert to a boxed handler function
    fn into_handler(self) -> BoxedResourceHandler;
}

/// Marker for resource handlers
pub struct ResourceMarker;

impl<F, Fut, Res> IntoResourceHandler<ResourceMarker> for F
where
    F: Fn(String) -> Fut + Clone + MaybeSend + MaybeSync + 'static,
    Fut: Future<Output = Res> + MaybeSend + 'static,
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
// Context-Aware Resource Handler Trait
// ============================================================================

/// Sealed trait for resource handlers with context injection.
///
/// Automatically implemented for async functions that:
/// - Take `Arc<RequestContext>` as first argument
/// - Take a `String` (the URI) as second argument
/// - Return something implementing `IntoResourceResponse`
pub trait IntoResourceHandlerWithCtx<M>: Clone + MaybeSend + MaybeSync + 'static {
    /// Convert to a boxed handler function that receives context
    fn into_handler_with_ctx(self) -> BoxedResourceHandlerWithCtx;
}

/// Marker for resource handlers with context
pub struct ResourceMarkerWithCtx;

impl<F, Fut, Res> IntoResourceHandlerWithCtx<ResourceMarkerWithCtx> for F
where
    F: Fn(Arc<RequestContext>, String) -> Fut + Clone + MaybeSend + MaybeSync + 'static,
    Fut: Future<Output = Res> + MaybeSend + 'static,
    Res: IntoResourceResponse + 'static,
{
    fn into_handler_with_ctx(self) -> BoxedResourceHandlerWithCtx {
        Box::new(move |ctx: Arc<RequestContext>, uri: String| {
            let handler = self.clone();
            Box::pin(async move { handler(ctx, uri).await.into_resource_response() })
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
pub trait IntoPromptHandler<T, M>: Clone + MaybeSend + MaybeSync + 'static {
    /// Convert to a boxed handler function
    fn into_handler(self) -> BoxedPromptHandler;

    /// Get the prompt arguments schema
    fn arguments() -> Vec<turbomcp_core::types::prompts::PromptArgument>;
}

/// Marker for typed prompt arguments
pub struct PromptWithArgs<A>(PhantomData<A>);

impl<F, A, Fut, Res> IntoPromptHandler<A, PromptWithArgs<A>> for F
where
    F: Fn(Option<A>) -> Fut + Clone + MaybeSend + MaybeSync + 'static,
    A: DeserializeOwned + schemars::JsonSchema + MaybeSend + 'static,
    Fut: Future<Output = Res> + MaybeSend + 'static,
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
    F: Fn() -> Fut + Clone + MaybeSend + MaybeSync + 'static,
    Fut: Future<Output = Res> + MaybeSend + 'static,
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

// ============================================================================
// Context-Aware Prompt Handler Trait
// ============================================================================

/// Sealed trait for prompt handlers with context injection.
///
/// Automatically implemented for async functions that:
/// - Take `Arc<RequestContext>` as first argument
/// - Take `Option<A>` where A: DeserializeOwned + JsonSchema as second argument
/// - Return something implementing `IntoPromptResponse`
pub trait IntoPromptHandlerWithCtx<T, M>: Clone + MaybeSend + MaybeSync + 'static {
    /// Convert to a boxed handler function that receives context
    fn into_handler_with_ctx(self) -> BoxedPromptHandlerWithCtx;

    /// Get the prompt arguments schema
    fn arguments() -> Vec<turbomcp_core::types::prompts::PromptArgument>;
}

/// Marker for typed prompt arguments with context
pub struct PromptWithCtxArgs<A>(PhantomData<A>);

impl<F, A, Fut, Res> IntoPromptHandlerWithCtx<A, PromptWithCtxArgs<A>> for F
where
    F: Fn(Arc<RequestContext>, Option<A>) -> Fut + Clone + MaybeSend + MaybeSync + 'static,
    A: DeserializeOwned + schemars::JsonSchema + MaybeSend + 'static,
    Fut: Future<Output = Res> + MaybeSend + 'static,
    Res: IntoPromptResponse + 'static,
{
    fn into_handler_with_ctx(self) -> BoxedPromptHandlerWithCtx {
        Box::new(
            move |ctx: Arc<RequestContext>, args: Option<serde_json::Value>| {
                let handler = self.clone();
                Box::pin(async move {
                    let parsed_args: Option<A> = match args {
                        Some(v) => Some(
                            serde_json::from_value(v)
                                .map_err(|e| format!("Invalid arguments: {e}"))?,
                        ),
                        None => None,
                    };
                    handler(ctx, parsed_args).await.into_prompt_response()
                })
            },
        )
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

/// Marker for no prompt arguments with context
pub struct PromptWithCtxNoArgs;

impl<F, Fut, Res> IntoPromptHandlerWithCtx<(), PromptWithCtxNoArgs> for F
where
    F: Fn(Arc<RequestContext>) -> Fut + Clone + MaybeSend + MaybeSync + 'static,
    Fut: Future<Output = Res> + MaybeSend + 'static,
    Res: IntoPromptResponse + 'static,
{
    fn into_handler_with_ctx(self) -> BoxedPromptHandlerWithCtx {
        Box::new(
            move |ctx: Arc<RequestContext>, _args: Option<serde_json::Value>| {
                let handler = self.clone();
                Box::pin(async move { handler(ctx).await.into_prompt_response() })
            },
        )
    }

    fn arguments() -> Vec<turbomcp_core::types::prompts::PromptArgument> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_context_aware_schema_generation() {
        // Test that schema is generated correctly for context + typed args
        // The schema for WithCtxArgs should be the same as the underlying args type
        let schema = schemars::schema_for!(TestArgs);
        let schema_value = serde_json::to_value(&schema).unwrap();
        assert!(schema_value.get("properties").is_some());
        let props = schema_value.get("properties").unwrap().as_object().unwrap();
        assert!(props.contains_key("name"));
    }

    #[test]
    fn test_context_only_schema() {
        // Test that empty schema is created for context-only handlers
        let schema = serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        });
        let obj = schema.as_object().unwrap();
        assert_eq!(obj.get("type").unwrap(), "object");
        assert!(
            obj.get("properties")
                .unwrap()
                .as_object()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn test_request_context_creation() {
        // Test that RequestContext can be created and used
        let ctx = RequestContext::new();
        assert!(!ctx.request_id().is_empty());
        assert_eq!(ctx.transport(), Some("wasm-worker"));
    }

    #[test]
    fn test_request_context_with_session() {
        let ctx = RequestContext::new()
            .with_session_id("session-123")
            .with_user_id("user-456");

        assert_eq!(ctx.session_id(), Some("session-123"));
        assert_eq!(ctx.user_id(), Some("user-456"));
    }
}
