//! Conversion traits for ergonomic result handling.
//!
//! These traits allow tool, resource, and prompt handlers to return
//! various types that are automatically converted to the appropriate result type.
//!
//! # Example
//!
//! ```
//! use turbomcp_types::{IntoToolResult, ToolResult};
//!
//! // All of these work as tool return types:
//! fn returns_string() -> impl IntoToolResult { "Hello".to_string() }
//! fn returns_i64() -> impl IntoToolResult { 42i64 }
//! fn returns_result() -> impl IntoToolResult { Ok::<_, String>("Success") }
//! fn returns_tool_result() -> impl IntoToolResult { ToolResult::text("Direct") }
//! ```

use std::fmt::Display;

use serde::Serialize;

use crate::content::Message;
use crate::results::{PromptResult, ResourceResult, ToolResult};

/// Convert any type to a `ToolResult`.
///
/// This trait is implemented for common types, allowing handlers to return
/// simple types that are automatically wrapped in `ToolResult`.
///
/// # Implementations
///
/// - `String`, `&str` → text result
/// - Numeric types → text result with string representation
/// - `bool` → text result ("true" or "false")
/// - `()` → empty result
/// - `ToolResult` → pass through
/// - `Result<T, E>` → success result or error result
/// - `Option<T>` → result or empty
/// - `Vec<T>` → JSON result
///
/// # Example
///
/// ```
/// use turbomcp_types::{IntoToolResult, ToolResult};
///
/// // String becomes text result
/// let result: ToolResult = "Hello".to_string().into_tool_result();
/// assert_eq!(result.first_text(), Some("Hello"));
///
/// // Numbers become text
/// let result: ToolResult = 42i64.into_tool_result();
/// assert_eq!(result.first_text(), Some("42"));
///
/// // Results are handled properly
/// let ok: Result<&str, &str> = Ok("success");
/// let result = ok.into_tool_result();
/// assert!(!result.is_error());
///
/// let err: Result<&str, &str> = Err("failed");
/// let result = err.into_tool_result();
/// assert!(result.is_error());
/// ```
pub trait IntoToolResult {
    /// Convert this value into a `ToolResult`.
    fn into_tool_result(self) -> ToolResult;
}

// String types
impl IntoToolResult for String {
    fn into_tool_result(self) -> ToolResult {
        ToolResult::text(self)
    }
}

impl IntoToolResult for &str {
    fn into_tool_result(self) -> ToolResult {
        ToolResult::text(self)
    }
}

impl IntoToolResult for &String {
    fn into_tool_result(self) -> ToolResult {
        ToolResult::text(self.clone())
    }
}

// Numeric types
impl IntoToolResult for i8 {
    fn into_tool_result(self) -> ToolResult {
        ToolResult::text(self.to_string())
    }
}

impl IntoToolResult for i16 {
    fn into_tool_result(self) -> ToolResult {
        ToolResult::text(self.to_string())
    }
}

impl IntoToolResult for i32 {
    fn into_tool_result(self) -> ToolResult {
        ToolResult::text(self.to_string())
    }
}

impl IntoToolResult for i64 {
    fn into_tool_result(self) -> ToolResult {
        ToolResult::text(self.to_string())
    }
}

impl IntoToolResult for i128 {
    fn into_tool_result(self) -> ToolResult {
        ToolResult::text(self.to_string())
    }
}

impl IntoToolResult for isize {
    fn into_tool_result(self) -> ToolResult {
        ToolResult::text(self.to_string())
    }
}

impl IntoToolResult for u8 {
    fn into_tool_result(self) -> ToolResult {
        ToolResult::text(self.to_string())
    }
}

impl IntoToolResult for u16 {
    fn into_tool_result(self) -> ToolResult {
        ToolResult::text(self.to_string())
    }
}

impl IntoToolResult for u32 {
    fn into_tool_result(self) -> ToolResult {
        ToolResult::text(self.to_string())
    }
}

impl IntoToolResult for u64 {
    fn into_tool_result(self) -> ToolResult {
        ToolResult::text(self.to_string())
    }
}

impl IntoToolResult for u128 {
    fn into_tool_result(self) -> ToolResult {
        ToolResult::text(self.to_string())
    }
}

impl IntoToolResult for usize {
    fn into_tool_result(self) -> ToolResult {
        ToolResult::text(self.to_string())
    }
}

impl IntoToolResult for f32 {
    fn into_tool_result(self) -> ToolResult {
        ToolResult::text(self.to_string())
    }
}

impl IntoToolResult for f64 {
    fn into_tool_result(self) -> ToolResult {
        ToolResult::text(self.to_string())
    }
}

// Boolean
impl IntoToolResult for bool {
    fn into_tool_result(self) -> ToolResult {
        ToolResult::text(self.to_string())
    }
}

// Unit type (empty result)
impl IntoToolResult for () {
    fn into_tool_result(self) -> ToolResult {
        ToolResult::empty()
    }
}

// Pass through
impl IntoToolResult for ToolResult {
    fn into_tool_result(self) -> ToolResult {
        self
    }
}

// Result handling
impl<T: IntoToolResult, E: Display> IntoToolResult for Result<T, E> {
    fn into_tool_result(self) -> ToolResult {
        match self {
            Ok(v) => v.into_tool_result(),
            Err(e) => ToolResult::error(e.to_string()),
        }
    }
}

// Option handling
impl<T: IntoToolResult> IntoToolResult for Option<T> {
    fn into_tool_result(self) -> ToolResult {
        match self {
            Some(v) => v.into_tool_result(),
            None => ToolResult::empty(),
        }
    }
}

// Vec as JSON (for serializable types)
impl<T: Serialize> IntoToolResult for Vec<T> {
    fn into_tool_result(self) -> ToolResult {
        ToolResult::json(&self).unwrap_or_else(|e| ToolResult::error(e.to_string()))
    }
}

// JSON Value
impl IntoToolResult for serde_json::Value {
    fn into_tool_result(self) -> ToolResult {
        ToolResult::json(&self).unwrap_or_else(|e| ToolResult::error(e.to_string()))
    }
}

/// Convert any type to a `ResourceResult`.
///
/// This trait allows resource handlers to return simple types that are
/// automatically wrapped in `ResourceResult`.
///
/// # Example
///
/// ```
/// use turbomcp_types::{IntoResourceResult, ResourceResult};
///
/// // String becomes text resource
/// let result: ResourceResult = "Content".to_string().into_resource_result("file:///test");
/// assert_eq!(result.first_text(), Some("Content"));
/// ```
pub trait IntoResourceResult {
    /// Convert this value into a `ResourceResult`.
    ///
    /// The `uri` parameter is used to set the resource URI.
    fn into_resource_result(self, uri: &str) -> ResourceResult;
}

impl IntoResourceResult for String {
    fn into_resource_result(self, uri: &str) -> ResourceResult {
        ResourceResult::text(uri, self)
    }
}

impl IntoResourceResult for &str {
    fn into_resource_result(self, uri: &str) -> ResourceResult {
        ResourceResult::text(uri, self)
    }
}

impl IntoResourceResult for ResourceResult {
    fn into_resource_result(self, _uri: &str) -> ResourceResult {
        self
    }
}

impl<T: IntoResourceResult, E: Display> IntoResourceResult for Result<T, E> {
    fn into_resource_result(self, uri: &str) -> ResourceResult {
        match self {
            Ok(v) => v.into_resource_result(uri),
            Err(e) => ResourceResult::text(uri, format!("Error: {}", e)),
        }
    }
}

impl<T: IntoResourceResult> IntoResourceResult for Option<T> {
    fn into_resource_result(self, uri: &str) -> ResourceResult {
        match self {
            Some(v) => v.into_resource_result(uri),
            None => ResourceResult::empty(),
        }
    }
}

/// Convert any type to a `PromptResult`.
///
/// This trait allows prompt handlers to return various message types
/// that are automatically wrapped in `PromptResult`.
///
/// # Example
///
/// ```
/// use turbomcp_types::{IntoPromptResult, PromptResult, Message};
///
/// // Vec of messages becomes prompt
/// let messages = vec![Message::user("Hello"), Message::assistant("Hi!")];
/// let result: PromptResult = messages.into_prompt_result();
/// assert_eq!(result.len(), 2);
/// ```
pub trait IntoPromptResult {
    /// Convert this value into a `PromptResult`.
    fn into_prompt_result(self) -> PromptResult;
}

impl IntoPromptResult for Vec<Message> {
    fn into_prompt_result(self) -> PromptResult {
        PromptResult::new(self)
    }
}

impl IntoPromptResult for PromptResult {
    fn into_prompt_result(self) -> PromptResult {
        self
    }
}

impl IntoPromptResult for Message {
    fn into_prompt_result(self) -> PromptResult {
        PromptResult::new(vec![self])
    }
}

impl IntoPromptResult for String {
    fn into_prompt_result(self) -> PromptResult {
        PromptResult::user(self)
    }
}

impl IntoPromptResult for &str {
    fn into_prompt_result(self) -> PromptResult {
        PromptResult::user(self)
    }
}

impl<T: IntoPromptResult, E: Display> IntoPromptResult for Result<T, E> {
    fn into_prompt_result(self) -> PromptResult {
        match self {
            Ok(v) => v.into_prompt_result(),
            // Note: Errors are converted to user messages for compatibility.
            // For proper error propagation, return McpResult from handlers.
            Err(e) => PromptResult::user(format!("Error: {}", e)),
        }
    }
}

impl<T: IntoPromptResult> IntoPromptResult for Option<T> {
    fn into_prompt_result(self) -> PromptResult {
        match self {
            Some(v) => v.into_prompt_result(),
            None => PromptResult::empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_into_tool_result() {
        let result = "Hello".to_string().into_tool_result();
        assert_eq!(result.first_text(), Some("Hello"));
        assert!(!result.is_error());
    }

    #[test]
    fn test_str_into_tool_result() {
        let result = "Hello".into_tool_result();
        assert_eq!(result.first_text(), Some("Hello"));
    }

    #[test]
    fn test_i64_into_tool_result() {
        let result = 42i64.into_tool_result();
        assert_eq!(result.first_text(), Some("42"));
    }

    #[test]
    fn test_bool_into_tool_result() {
        let result = true.into_tool_result();
        assert_eq!(result.first_text(), Some("true"));
    }

    #[test]
    fn test_unit_into_tool_result() {
        let result = ().into_tool_result();
        assert!(result.content.is_empty());
    }

    #[test]
    fn test_result_ok_into_tool_result() {
        let r: Result<&str, &str> = Ok("success");
        let result = r.into_tool_result();
        assert_eq!(result.first_text(), Some("success"));
        assert!(!result.is_error());
    }

    #[test]
    fn test_result_err_into_tool_result() {
        let r: Result<&str, &str> = Err("failed");
        let result = r.into_tool_result();
        assert_eq!(result.first_text(), Some("failed"));
        assert!(result.is_error());
    }

    #[test]
    fn test_option_some_into_tool_result() {
        let r: Option<&str> = Some("value");
        let result = r.into_tool_result();
        assert_eq!(result.first_text(), Some("value"));
    }

    #[test]
    fn test_option_none_into_tool_result() {
        let r: Option<&str> = None;
        let result = r.into_tool_result();
        assert!(result.content.is_empty());
    }

    #[test]
    fn test_vec_into_tool_result() {
        let v = vec!["a", "b", "c"];
        let result = v.into_tool_result();
        assert!(result.structured_content.is_some());
    }

    #[test]
    fn test_string_into_resource_result() {
        let result = "content".to_string().into_resource_result("file:///test");
        assert_eq!(result.first_text(), Some("content"));
        match &result.contents[0] {
            crate::content::ResourceContents::Text(t) => assert_eq!(t.uri, "file:///test"),
            _ => panic!("Expected text resource contents"),
        }
    }

    #[test]
    fn test_messages_into_prompt_result() {
        let messages = vec![Message::user("Hello")];
        let result = messages.into_prompt_result();
        assert_eq!(result.len(), 1);
    }
}
