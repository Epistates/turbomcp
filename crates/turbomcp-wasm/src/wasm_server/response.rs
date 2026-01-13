//! Response traits and types for ergonomic tool handler returns
//!
//! This module provides the `IntoToolResponse` trait, inspired by axum's `IntoResponse`,
//! allowing handlers to return various types that can be converted into `ToolResult`.
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

use serde::Serialize;
use std::fmt::Display;

use super::types::ToolResult;
use turbomcp_core::types::content::Content;

/// Trait for types that can be converted into a tool response.
///
/// This is the primary trait for ergonomic tool handler returns.
/// Implement this trait to allow your types to be returned directly from handlers.
///
/// # Built-in Implementations
///
/// - `String`, `&str` - Returns as text content
/// - `ToolResult` - Passed through as-is
/// - `Json<T>` - Serializes to JSON text
/// - `Result<T, E>` where `T: IntoToolResponse`, `E: Into<ToolError>` - Handles errors automatically
/// - `()` - Returns empty success response
///
/// # Example
///
/// ```ignore
/// // Simple string return
/// async fn handler(_: Args) -> impl IntoToolResponse {
///     "Hello, world!"
/// }
///
/// // Automatic error handling
/// async fn handler(_: Args) -> Result<String, ToolError> {
///     let data = fallible_operation()?;
///     Ok(format!("Got: {}", data))
/// }
/// ```
pub trait IntoToolResponse {
    /// Convert this type into a `ToolResult`
    fn into_tool_response(self) -> ToolResult;
}

// ============================================================================
// Core implementations
// ============================================================================

impl IntoToolResponse for ToolResult {
    #[inline]
    fn into_tool_response(self) -> ToolResult {
        self
    }
}

impl IntoToolResponse for String {
    #[inline]
    fn into_tool_response(self) -> ToolResult {
        ToolResult::text(self)
    }
}

impl IntoToolResponse for &str {
    #[inline]
    fn into_tool_response(self) -> ToolResult {
        ToolResult::text(self)
    }
}

impl IntoToolResponse for () {
    #[inline]
    fn into_tool_response(self) -> ToolResult {
        ToolResult {
            content: vec![],
            is_error: None,
        }
    }
}

// Numeric type implementations
macro_rules! impl_into_tool_response_for_numeric {
    ($($t:ty),*) => {
        $(
            impl IntoToolResponse for $t {
                #[inline]
                fn into_tool_response(self) -> ToolResult {
                    ToolResult::text(self.to_string())
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
    fn into_tool_response(self) -> ToolResult {
        ToolResult::text(self.to_string())
    }
}

impl IntoToolResponse for Content {
    #[inline]
    fn into_tool_response(self) -> ToolResult {
        ToolResult {
            content: vec![self],
            is_error: None,
        }
    }
}

impl IntoToolResponse for Vec<Content> {
    #[inline]
    fn into_tool_response(self) -> ToolResult {
        ToolResult::contents(self)
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
    fn into_tool_response(self) -> ToolResult {
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
/// use turbomcp_wasm::wasm_server::Json;
///
/// #[derive(Serialize)]
/// struct UserData {
///     name: String,
///     age: u32,
/// }
///
/// async fn get_user(_: Args) -> impl IntoToolResponse {
///     Json(UserData {
///         name: "Alice".into(),
///         age: 30,
///     })
/// }
/// ```
#[derive(Debug, Clone)]
pub struct Json<T>(pub T);

impl<T: Serialize> IntoToolResponse for Json<T> {
    fn into_tool_response(self) -> ToolResult {
        match serde_json::to_string_pretty(&self.0) {
            Ok(json) => ToolResult::text(json),
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
/// async fn handler(_: Args) -> impl IntoToolResponse {
///     Text("Operation completed successfully")
/// }
/// ```
#[derive(Debug, Clone)]
pub struct Text<T>(pub T);

impl<T: Into<String>> IntoToolResponse for Text<T> {
    #[inline]
    fn into_tool_response(self) -> ToolResult {
        ToolResult::text(self.0)
    }
}

/// Wrapper for returning base64-encoded image data.
///
/// # Example
///
/// ```ignore
/// async fn get_image(_: Args) -> impl IntoToolResponse {
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
    fn into_tool_response(self) -> ToolResult {
        ToolResult::image(self.data, self.mime_type)
    }
}

// ============================================================================
// Error handling
// ============================================================================

/// Error type for tool handlers that supports the `?` operator.
///
/// This type can be created from any error that implements `std::error::Error`
/// or `Display`, allowing idiomatic Rust error handling in tool handlers.
///
/// # Example
///
/// ```ignore
/// use turbomcp_wasm::wasm_server::ToolError;
///
/// async fn handler(args: Args) -> Result<String, ToolError> {
///     // Use ? operator - errors automatically convert to ToolError
///     let file = std::fs::read_to_string(&args.path)?;
///     let data: MyData = serde_json::from_str(&file)?;
///     Ok(format!("Loaded: {:?}", data))
/// }
///
/// // Create errors manually
/// async fn validate(args: Args) -> Result<String, ToolError> {
///     if args.value < 0 {
///         return Err(ToolError::new("Value must be non-negative"));
///     }
///     Ok("Valid".into())
/// }
/// ```
#[derive(Debug, Clone)]
pub struct ToolError {
    message: String,
}

impl ToolError {
    /// Create a new tool error with the given message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl IntoToolResponse for ToolError {
    #[inline]
    fn into_tool_response(self) -> ToolResult {
        ToolResult::error(self.message)
    }
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ToolError {}

// Implement From for common error types to enable ? operator
// We can't use a blanket impl because it conflicts with From<T> for T

impl From<std::io::Error> for ToolError {
    fn from(e: std::io::Error) -> Self {
        Self {
            message: e.to_string(),
        }
    }
}

impl From<serde_json::Error> for ToolError {
    fn from(e: serde_json::Error) -> Self {
        Self {
            message: e.to_string(),
        }
    }
}

impl From<std::string::FromUtf8Error> for ToolError {
    fn from(e: std::string::FromUtf8Error) -> Self {
        Self {
            message: e.to_string(),
        }
    }
}

impl From<std::num::ParseIntError> for ToolError {
    fn from(e: std::num::ParseIntError) -> Self {
        Self {
            message: e.to_string(),
        }
    }
}

impl From<std::num::ParseFloatError> for ToolError {
    fn from(e: std::num::ParseFloatError) -> Self {
        Self {
            message: e.to_string(),
        }
    }
}

impl From<&str> for ToolError {
    fn from(s: &str) -> Self {
        Self {
            message: s.to_string(),
        }
    }
}

impl From<String> for ToolError {
    fn from(s: String) -> Self {
        Self { message: s }
    }
}

impl From<Box<dyn std::error::Error>> for ToolError {
    fn from(e: Box<dyn std::error::Error>) -> Self {
        Self {
            message: e.to_string(),
        }
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for ToolError {
    fn from(e: Box<dyn std::error::Error + Send + Sync>) -> Self {
        Self {
            message: e.to_string(),
        }
    }
}

/// Convenience trait for converting to ToolError
///
/// Provides `.tool_err()` method for easy error conversion with custom messages.
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
    fn into_tool_response(self) -> ToolResult {
        let a = self.0.into_tool_response();
        let b = self.1.into_tool_response();

        let mut content = a.content;
        content.extend(b.content);

        ToolResult {
            content,
            is_error: a.is_error.or(b.is_error),
        }
    }
}

// ============================================================================
// Option implementation
// ============================================================================

impl<T: IntoToolResponse> IntoToolResponse for Option<T> {
    fn into_tool_response(self) -> ToolResult {
        match self {
            Some(v) => v.into_tool_response(),
            None => ToolResult::text("No result"),
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
