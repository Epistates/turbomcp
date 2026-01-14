//! Response traits and types for ergonomic tool handler returns
//!
//! This module re-exports the unified `IntoToolResponse` trait from `turbomcp-core`,
//! allowing handlers to return various types that can be converted into tool results.
//!
//! # Unified Handler Architecture
//!
//! As of TurboMCP v3, the response traits are unified across WASM and native targets.
//! This means you can write handlers once and run them on both:
//! - WASM environments (Cloudflare Workers, Deno Deploy, etc.)
//! - Native environments (stdio, HTTP, TCP, etc.)
//!
//! # Example
//!
//! ```ignore
//! use turbomcp_wasm::wasm_server::*;
//!
//! // Return a simple string
//! async fn greet(args: Args) -> impl IntoToolResponse {
//!     format!("Hello, {}!", args.name)
//! }
//!
//! // Return JSON with automatic serialization
//! async fn get_data(args: Args) -> impl IntoToolResponse {
//!     Json(MyData { value: 42 })
//! }
//!
//! // Use ? operator with automatic error conversion
//! async fn fetch_data(args: Args) -> Result<Json<Data>, ToolError> {
//!     let data = some_fallible_operation()?;
//!     Ok(Json(data))
//! }
//! ```
//!
//! # Handler Return Types
//!
//! Tool handlers can return any type that implements `IntoToolResponse`:
//!
//! | Type | Result |
//! |------|--------|
//! | `String`, `&str` | Text content |
//! | `Json<T>` | Pretty-printed JSON text |
//! | `Text<T>` | Explicit text content |
//! | `Image<D, M>` | Base64 image with MIME type |
//! | `CallToolResult` / `ToolResult` | Full control over response |
//! | `Result<T, E>` | Automatic error handling with `?` |
//! | `Option<T>` | `None` returns "No result" |
//! | `()` | Empty success response |
//! | Numeric types | Stringified number |
//! | `bool` | "true" or "false" |
//! | `(A, B)` | Combined content from both |

// Re-export all response types from turbomcp-core
// This provides a unified API across WASM and native targets
pub use turbomcp_core::response::{Image, IntoToolError, IntoToolResponse, Json, Text, ToolError};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wasm_server::types::ToolResult;

    #[test]
    fn test_string_into_response() {
        let response = "hello".into_tool_response();
        assert_eq!(response.content.len(), 1);
        assert!(response.is_error.is_none());
    }

    #[test]
    fn test_owned_string_into_response() {
        let response = String::from("hello").into_tool_response();
        assert_eq!(response.content.len(), 1);
    }

    #[test]
    fn test_json_into_response() {
        let data = serde_json::json!({"key": "value"});
        let response = Json(data).into_tool_response();
        assert_eq!(response.content.len(), 1);
    }

    #[test]
    fn test_tool_error_into_response() {
        let error = ToolError::new("something went wrong");
        let response = error.into_tool_response();
        assert_eq!(response.is_error, Some(true));
    }

    #[test]
    fn test_result_ok_into_response() {
        let result: Result<String, ToolError> = Ok("success".into());
        let response = result.into_tool_response();
        assert!(response.is_error.is_none());
    }

    #[test]
    fn test_result_err_into_response() {
        let result: Result<String, ToolError> = Err(ToolError::new("failed"));
        let response = result.into_tool_response();
        assert_eq!(response.is_error, Some(true));
    }

    #[test]
    fn test_unit_into_response() {
        let response = ().into_tool_response();
        assert!(response.content.is_empty());
    }

    #[test]
    fn test_option_some_into_response() {
        let response = Some("value").into_tool_response();
        assert_eq!(response.content.len(), 1);
    }

    #[test]
    fn test_option_none_into_response() {
        let response: ToolResult = None::<String>.into_tool_response();
        assert_eq!(response.content.len(), 1);
    }

    #[test]
    fn test_tuple_into_response() {
        let response = ("first", "second").into_tool_response();
        assert_eq!(response.content.len(), 2);
    }

    #[test]
    fn test_text_wrapper() {
        let response = Text("explicit text").into_tool_response();
        assert_eq!(response.content.len(), 1);
    }

    #[test]
    fn test_image_wrapper() {
        let response = Image {
            data: "base64data",
            mime_type: "image/png",
        }
        .into_tool_response();
        assert_eq!(response.content.len(), 1);
    }
}
