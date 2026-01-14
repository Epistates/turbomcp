//! Response traits for ergonomic tool handler returns.
//!
//! This module provides the `IntoToolResponse` trait, inspired by axum's `IntoResponse`,
//! allowing handlers to return various types that can be converted into `CallToolResult`.
//!
//! # Features
//!
//! - `no_std` compatible (uses `alloc`)
//! - Automatic conversion from common types (String, numbers, bool, etc.)
//! - Result and Option support for error handling with `?` operator
//! - Wrapper types for explicit control (Json, Text, Image)
//!
//! # Example
//!
//! ```ignore
//! use turbomcp_core::response::IntoToolResponse;
//!
//! // Return a simple string
//! async fn greet(name: String) -> impl IntoToolResponse {
//!     format!("Hello, {}!", name)
//! }
//!
//! // Return JSON with automatic serialization
//! async fn get_data() -> impl IntoToolResponse {
//!     Json(MyData { value: 42 })
//! }
//!
//! // Use ? operator with automatic error conversion
//! async fn fetch_data() -> Result<String, ToolError> {
//!     let data = some_fallible_operation()?;
//!     Ok(format!("Got: {}", data))
//! }
//! ```

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::Display;

use serde::Serialize;

use crate::types::content::Content;
use crate::types::tools::CallToolResult;

/// Trait for types that can be converted into a tool response.
///
/// This is the primary trait for ergonomic tool handler returns.
/// Implement this trait to allow your types to be returned directly from handlers.
///
/// # Built-in Implementations
///
/// - `String`, `&str` - Returns as text content
/// - `CallToolResult` - Passed through as-is
/// - `Json<T>` - Serializes to JSON text
/// - `Result<T, E>` where `T: IntoToolResponse`, `E: Into<ToolError>` - Handles errors automatically
/// - `()` - Returns empty success response
/// - Numeric types (`i32`, `i64`, `f64`, etc.) - Returns as text
/// - `bool` - Returns as "true" or "false"
///
/// # Example
///
/// ```ignore
/// // Simple string return
/// async fn handler() -> impl IntoToolResponse {
///     "Hello, world!"
/// }
///
/// // Automatic error handling
/// async fn handler() -> Result<String, ToolError> {
///     let data = fallible_operation()?;
///     Ok(format!("Got: {}", data))
/// }
/// ```
pub trait IntoToolResponse {
    /// Convert this type into a `CallToolResult`
    fn into_tool_response(self) -> CallToolResult;
}

// ============================================================================
// Core implementations
// ============================================================================

impl IntoToolResponse for CallToolResult {
    #[inline]
    fn into_tool_response(self) -> CallToolResult {
        self
    }
}

impl IntoToolResponse for String {
    #[inline]
    fn into_tool_response(self) -> CallToolResult {
        CallToolResult::text(self)
    }
}

impl IntoToolResponse for &str {
    #[inline]
    fn into_tool_response(self) -> CallToolResult {
        CallToolResult::text(self)
    }
}

impl IntoToolResponse for () {
    #[inline]
    fn into_tool_response(self) -> CallToolResult {
        CallToolResult {
            content: vec![],
            is_error: None,
            _meta: None,
        }
    }
}

// Numeric type implementations
macro_rules! impl_into_tool_response_for_numeric {
    ($($t:ty),*) => {
        $(
            impl IntoToolResponse for $t {
                #[inline]
                fn into_tool_response(self) -> CallToolResult {
                    CallToolResult::text(self.to_string())
                }
            }
        )*
    };
}

impl_into_tool_response_for_numeric!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64
);

impl IntoToolResponse for bool {
    #[inline]
    fn into_tool_response(self) -> CallToolResult {
        CallToolResult::text(self.to_string())
    }
}

impl IntoToolResponse for Content {
    #[inline]
    fn into_tool_response(self) -> CallToolResult {
        CallToolResult {
            content: vec![self],
            is_error: None,
            _meta: None,
        }
    }
}

impl IntoToolResponse for Vec<Content> {
    #[inline]
    fn into_tool_response(self) -> CallToolResult {
        CallToolResult {
            content: self,
            is_error: None,
            _meta: None,
        }
    }
}

// ============================================================================
// Result implementations - enables ? operator
// ============================================================================

impl<T, E> IntoToolResponse for Result<T, E>
where
    T: IntoToolResponse,
    E: Into<ToolError>,
{
    fn into_tool_response(self) -> CallToolResult {
        match self {
            Ok(v) => v.into_tool_response(),
            Err(e) => {
                let error: ToolError = e.into();
                error.into_tool_response()
            }
        }
    }
}

// ============================================================================
// Convenience wrapper types
// ============================================================================

/// Wrapper for returning JSON-serialized data from a tool handler.
///
/// Automatically serializes the inner value to pretty-printed JSON.
///
/// # Example
///
/// ```ignore
/// use turbomcp_core::response::Json;
///
/// #[derive(Serialize)]
/// struct UserData {
///     name: String,
///     age: u32,
/// }
///
/// async fn get_user() -> impl IntoToolResponse {
///     Json(UserData {
///         name: "Alice".into(),
///         age: 30,
///     })
/// }
/// ```
#[derive(Debug, Clone)]
pub struct Json<T>(pub T);

impl<T: Serialize> IntoToolResponse for Json<T> {
    fn into_tool_response(self) -> CallToolResult {
        match serde_json::to_string_pretty(&self.0) {
            Ok(json) => {
                // Enforce size limit to prevent DoS from oversized responses
                if json.len() > crate::MAX_MESSAGE_SIZE {
                    return ToolError::new(format!(
                        "JSON output too large: {} bytes exceeds {} byte limit",
                        json.len(),
                        crate::MAX_MESSAGE_SIZE
                    ))
                    .into_tool_response();
                }
                CallToolResult::text(json)
            }
            Err(e) => {
                ToolError::new(format!("JSON serialization failed: {e}")).into_tool_response()
            }
        }
    }
}

/// Wrapper for explicitly returning text content.
///
/// This is semantically equivalent to returning a `String`, but makes intent clearer.
///
/// # Example
///
/// ```ignore
/// async fn handler() -> impl IntoToolResponse {
///     Text("Operation completed successfully")
/// }
/// ```
#[derive(Debug, Clone)]
pub struct Text<T>(pub T);

impl<T: Into<String>> IntoToolResponse for Text<T> {
    #[inline]
    fn into_tool_response(self) -> CallToolResult {
        CallToolResult::text(self.0)
    }
}

/// Wrapper for returning base64-encoded image data.
///
/// # Example
///
/// ```ignore
/// async fn get_image() -> impl IntoToolResponse {
///     Image {
///         data: base64_encoded_png,
///         mime_type: "image/png",
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct Image<D, M> {
    /// Base64-encoded image data
    pub data: D,
    /// MIME type of the image (e.g., "image/png", "image/jpeg")
    pub mime_type: M,
}

impl<D: Into<String>, M: Into<String>> IntoToolResponse for Image<D, M> {
    #[inline]
    fn into_tool_response(self) -> CallToolResult {
        CallToolResult {
            content: vec![Content::image(self.data, self.mime_type)],
            is_error: None,
            _meta: None,
        }
    }
}

// ============================================================================
// Error handling
// ============================================================================

/// Error type for tool handlers that supports the `?` operator.
///
/// This type can be created from any error that implements `Display`,
/// allowing idiomatic Rust error handling in tool handlers.
///
/// # Example
///
/// ```ignore
/// use turbomcp_core::response::ToolError;
///
/// async fn handler(path: String) -> Result<String, ToolError> {
///     // Use ? operator - errors automatically convert to ToolError
///     let file = std::fs::read_to_string(&path)?;
///     Ok(format!("Read {} bytes", file.len()))
/// }
///
/// // Create errors manually
/// async fn validate(value: i32) -> Result<String, ToolError> {
///     if value < 0 {
///         return Err(ToolError::new("Value must be non-negative"));
///     }
///     Ok("Valid".into())
/// }
/// ```
#[derive(Debug, Clone)]
pub struct ToolError {
    message: String,
    code: Option<i32>,
}

impl ToolError {
    /// Create a new tool error with the given message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: None,
        }
    }

    /// Create a new tool error with a custom error code.
    pub fn with_code(code: i32, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: Some(code),
        }
    }

    /// Get the error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Get the error code, if any.
    pub fn code(&self) -> Option<i32> {
        self.code
    }
}

impl IntoToolResponse for ToolError {
    #[inline]
    fn into_tool_response(self) -> CallToolResult {
        CallToolResult::error(self.message)
    }
}

impl Display for ToolError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.message)
    }
}

// Note: std::error::Error requires std, so we only implement it when std is available
#[cfg(feature = "std")]
impl std::error::Error for ToolError {}

// ============================================================================
// From implementations for common error types
// ============================================================================

impl From<&str> for ToolError {
    fn from(s: &str) -> Self {
        Self {
            message: s.into(),
            code: None,
        }
    }
}

impl From<String> for ToolError {
    fn from(s: String) -> Self {
        Self {
            message: s,
            code: None,
        }
    }
}

impl From<serde_json::Error> for ToolError {
    fn from(e: serde_json::Error) -> Self {
        Self {
            message: e.to_string(),
            code: None,
        }
    }
}

// McpError conversion - enables McpResult<T> to work with IntoToolResponse
impl From<crate::error::McpError> for ToolError {
    fn from(e: crate::error::McpError) -> Self {
        Self {
            message: e.to_string(),
            code: Some(e.jsonrpc_code()),
        }
    }
}

// std-only error conversions
#[cfg(feature = "std")]
impl From<std::io::Error> for ToolError {
    fn from(e: std::io::Error) -> Self {
        Self {
            message: e.to_string(),
            code: None,
        }
    }
}

#[cfg(feature = "std")]
impl From<std::string::FromUtf8Error> for ToolError {
    fn from(e: std::string::FromUtf8Error) -> Self {
        Self {
            message: e.to_string(),
            code: None,
        }
    }
}

#[cfg(feature = "std")]
impl From<std::num::ParseIntError> for ToolError {
    fn from(e: std::num::ParseIntError) -> Self {
        Self {
            message: e.to_string(),
            code: None,
        }
    }
}

#[cfg(feature = "std")]
impl From<std::num::ParseFloatError> for ToolError {
    fn from(e: std::num::ParseFloatError) -> Self {
        Self {
            message: e.to_string(),
            code: None,
        }
    }
}

#[cfg(feature = "std")]
impl From<Box<dyn std::error::Error>> for ToolError {
    fn from(e: Box<dyn std::error::Error>) -> Self {
        Self {
            message: e.to_string(),
            code: None,
        }
    }
}

#[cfg(feature = "std")]
impl From<Box<dyn std::error::Error + Send + Sync>> for ToolError {
    fn from(e: Box<dyn std::error::Error + Send + Sync>) -> Self {
        Self {
            message: e.to_string(),
            code: None,
        }
    }
}

/// Convenience trait for converting to ToolError with context.
///
/// Provides `.tool_err()` method for easy error conversion with custom messages.
///
/// # Example
///
/// ```ignore
/// use turbomcp_core::response::IntoToolError;
///
/// fn process() -> Result<(), ToolError> {
///     some_operation()
///         .map_err(|e| e.tool_err("Failed to process"))?;
///     Ok(())
/// }
/// ```
pub trait IntoToolError {
    /// Convert to a ToolError with additional context
    fn tool_err(self, context: impl Display) -> ToolError;
}

impl<E: Display> IntoToolError for E {
    fn tool_err(self, context: impl Display) -> ToolError {
        ToolError::new(format!("{}: {}", context, self))
    }
}

// ============================================================================
// Tuple implementations for combining content
// ============================================================================

impl<A, B> IntoToolResponse for (A, B)
where
    A: IntoToolResponse,
    B: IntoToolResponse,
{
    fn into_tool_response(self) -> CallToolResult {
        let a = self.0.into_tool_response();
        let b = self.1.into_tool_response();

        let mut content = a.content;
        content.extend(b.content);

        CallToolResult {
            content,
            is_error: a.is_error.or(b.is_error),
            _meta: None,
        }
    }
}

// ============================================================================
// Option implementation
// ============================================================================

impl<T: IntoToolResponse> IntoToolResponse for Option<T> {
    fn into_tool_response(self) -> CallToolResult {
        match self {
            Some(v) => v.into_tool_response(),
            None => CallToolResult::text("No result"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let response: CallToolResult = None::<String>.into_tool_response();
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

    #[test]
    fn test_numeric_types() {
        assert_eq!(42i32.into_tool_response().content.len(), 1);
        assert_eq!(42i64.into_tool_response().content.len(), 1);
        assert_eq!(2.5f64.into_tool_response().content.len(), 1);
    }

    #[test]
    fn test_bool_into_response() {
        let true_response = true.into_tool_response();
        let false_response = false.into_tool_response();
        assert_eq!(true_response.content.len(), 1);
        assert_eq!(false_response.content.len(), 1);
    }

    #[test]
    fn test_json_size_limit_enforcement() {
        // Create JSON data larger than MAX_MESSAGE_SIZE (1MB)
        let large_string = "x".repeat(crate::MAX_MESSAGE_SIZE + 100);
        let large_data = serde_json::json!({ "data": large_string });
        let response = Json(large_data).into_tool_response();

        // Should return an error response
        assert_eq!(response.is_error, Some(true));
        assert_eq!(response.content.len(), 1);

        // Verify error message mentions size limit
        if let Content::Text { text, .. } = &response.content[0] {
            assert!(text.contains("too large"));
            assert!(text.contains("byte limit"));
        } else {
            panic!("Expected text content in error response");
        }
    }

    #[test]
    fn test_json_within_size_limit() {
        // Normal JSON should work fine
        let small_data = serde_json::json!({ "key": "value" });
        let response = Json(small_data).into_tool_response();

        // Should succeed
        assert!(response.is_error.is_none() || response.is_error == Some(false));
        assert_eq!(response.content.len(), 1);
    }
}
