//! Return-type conversions for macro-generated handlers.
//!
//! A `#[tool]` / `#[resource]` / `#[prompt]` method may return a plain `String`,
//! the neutral result type directly, or a fallible `McpResult<_>`. These traits
//! let the `#[server]` macro emit one uniform `.into_*()` call regardless, while
//! enforcing the spec's differing error semantics per method family:
//!
//! - **Tools**: a handler error becomes a `CallToolResult { is_error: true }`
//!   (the model sees and self-corrects — PLAN §4.11), *not* a JSON-RPC error.
//! - **Resources / prompts**: a handler error *propagates* as a JSON-RPC error,
//!   since `resources/read` and `prompts/get` have no `is_error` channel.

use serde::Serialize;
use turbomcp_core::{McpError, McpResult};
use turbomcp_protocol::neutral;

/// Convert a `#[tool]` return value into a [`neutral::CallToolResult`].
pub trait IntoCallToolResult {
    /// Perform the conversion.
    fn into_call_tool_result(self) -> McpResult<neutral::CallToolResult>;
}

/// Wrap a serializable value to return it from a `#[tool]` as **structured
/// output**: the value is placed in `structuredContent` and a JSON text mirror
/// is added to `content` for backward compatibility (the spec's recommended
/// shape for typed results).
///
/// When the `#[server]` macro sees a `-> Json<T>` return it also emits the
/// tool's `outputSchema` from `T` (requires `T: schemars::JsonSchema`).
///
/// ```ignore
/// #[derive(serde::Serialize, schemars::JsonSchema)]
/// struct Stats { count: u64, mean: f64 }
///
/// #[tool(description = "Compute stats")]
/// async fn stats(&self) -> Json<Stats> { Json(Stats { count: 3, mean: 1.5 }) }
/// ```
///
/// Note: on the `2025-11-25` wire, `structuredContent` must be a JSON object, so
/// a `Json<T>` whose `T` serializes to a non-object (a scalar or array) carries
/// its value only in the text mirror there; the `2026-07-28` wire accepts any
/// JSON value.
#[derive(Clone, Copy, Debug)]
pub struct Json<T>(pub T);

impl IntoCallToolResult for neutral::CallToolResult {
    fn into_call_tool_result(self) -> McpResult<neutral::CallToolResult> {
        Ok(self)
    }
}

impl IntoCallToolResult for String {
    fn into_call_tool_result(self) -> McpResult<neutral::CallToolResult> {
        Ok(neutral::CallToolResult::text(self))
    }
}

impl IntoCallToolResult for &str {
    fn into_call_tool_result(self) -> McpResult<neutral::CallToolResult> {
        Ok(neutral::CallToolResult::text(self))
    }
}

/// A tool that does work but returns nothing → a successful, empty-content
/// result (the spec permits an empty `content` array).
impl IntoCallToolResult for () {
    fn into_call_tool_result(self) -> McpResult<neutral::CallToolResult> {
        Ok(neutral::CallToolResult::default())
    }
}

/// Scalar returns become a single text block of their `Display` form. This is a
/// *concrete* set, not a blanket `impl<T: Display>`: a blanket would overlap the
/// `String`/`&str` impls (no specialization) and silently accept any `Display`
/// type. Structured/object data should use `Json` (→ `structuredContent`)
/// instead of a stringified scalar.
macro_rules! scalar_tool_result {
    ($($t:ty),* $(,)?) => {$(
        impl IntoCallToolResult for $t {
            fn into_call_tool_result(self) -> McpResult<neutral::CallToolResult> {
                Ok(neutral::CallToolResult::text(self.to_string()))
            }
        }
    )*};
}
scalar_tool_result!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64, bool,
);

/// Return an image from a `#[tool]`: base64-encoded `data` plus its `mime_type`
/// (e.g. `image/png`) become a single image content block.
///
/// ```ignore
/// #[tool(description = "Render a chart")]
/// async fn chart(&self) -> Image { Image { data: png_base64, mime_type: "image/png".into() } }
/// ```
#[derive(Clone, Debug)]
pub struct Image {
    /// Base64-encoded image bytes.
    pub data: String,
    /// The image MIME type.
    pub mime_type: String,
}

/// Return audio from a `#[tool]`: base64-encoded `data` plus its `mime_type`
/// (e.g. `audio/wav`) become a single audio content block.
#[derive(Clone, Debug)]
pub struct Audio {
    /// Base64-encoded audio bytes.
    pub data: String,
    /// The audio MIME type.
    pub mime_type: String,
}

impl IntoCallToolResult for Image {
    fn into_call_tool_result(self) -> McpResult<neutral::CallToolResult> {
        Ok(neutral::CallToolResult::new(vec![neutral::Content::image(
            self.data,
            self.mime_type,
        )]))
    }
}

impl IntoCallToolResult for Audio {
    fn into_call_tool_result(self) -> McpResult<neutral::CallToolResult> {
        Ok(neutral::CallToolResult::new(vec![neutral::Content::audio(
            self.data,
            self.mime_type,
        )]))
    }
}

impl<T: Serialize> IntoCallToolResult for Json<T> {
    fn into_call_tool_result(self) -> McpResult<neutral::CallToolResult> {
        let value = serde_json::to_value(&self.0)
            .map_err(|e| McpError::internal(format!("serializing Json tool result: {e}")))?;
        // Text mirror (spec backward-compat): the compact JSON rendering.
        let mirror = value.to_string();
        let mut result = neutral::CallToolResult::text(mirror);
        result.structured_content = Some(value);
        Ok(result)
    }
}

/// A fallible tool: an error becomes an `is_error` result (spec convention) —
/// its text is the tool-failure message.
///
/// Two errors are *protocol*-level and keep propagating instead, because they
/// say the call could not be made rather than that the tool ran and failed:
/// the MRTR abort sentinel (the dispatcher answers an `InputRequiredResult`),
/// and a missing client capability (SEP-2575 requires
/// `MissingRequiredClientCapabilityError`, which a `CallToolResult` cannot
/// express — a client that saw `isError` would have no way to learn it needs
/// to re-declare and retry).
impl<T> IntoCallToolResult for McpResult<T>
where
    T: IntoCallToolResult,
{
    fn into_call_tool_result(self) -> McpResult<neutral::CallToolResult> {
        match self {
            Ok(v) => v.into_call_tool_result(),
            Err(e @ (McpError::InputRequired | McpError::MissingRequiredCapability(_))) => Err(e),
            Err(e) => Ok(neutral::CallToolResult::error(e.to_string())),
        }
    }
}

/// Convert a `#[resource]` return value into a [`neutral::ReadResourceResult`],
/// using the resolved `uri` when wrapping bare text.
pub trait IntoReadResourceResult {
    /// Perform the conversion. `uri` is the resource being read.
    fn into_read_resource_result(self, uri: &str) -> McpResult<neutral::ReadResourceResult>;
}

impl IntoReadResourceResult for neutral::ReadResourceResult {
    fn into_read_resource_result(self, _uri: &str) -> McpResult<neutral::ReadResourceResult> {
        Ok(self)
    }
}

impl IntoReadResourceResult for String {
    fn into_read_resource_result(self, uri: &str) -> McpResult<neutral::ReadResourceResult> {
        Ok(neutral::ReadResourceResult::text(uri, self))
    }
}

impl IntoReadResourceResult for &str {
    fn into_read_resource_result(self, uri: &str) -> McpResult<neutral::ReadResourceResult> {
        Ok(neutral::ReadResourceResult::text(uri, self))
    }
}

/// A fallible resource read: the error *propagates* (no `is_error` channel).
impl<T> IntoReadResourceResult for McpResult<T>
where
    T: IntoReadResourceResult,
{
    fn into_read_resource_result(self, uri: &str) -> McpResult<neutral::ReadResourceResult> {
        self.and_then(|v| v.into_read_resource_result(uri))
    }
}

/// Convert a `#[prompt]` return value into a [`neutral::GetPromptResult`]. A bare
/// string becomes a single user message.
pub trait IntoGetPromptResult {
    /// Perform the conversion.
    fn into_get_prompt_result(self) -> McpResult<neutral::GetPromptResult>;
}

impl IntoGetPromptResult for neutral::GetPromptResult {
    fn into_get_prompt_result(self) -> McpResult<neutral::GetPromptResult> {
        Ok(self)
    }
}

impl IntoGetPromptResult for String {
    fn into_get_prompt_result(self) -> McpResult<neutral::GetPromptResult> {
        Ok(neutral::GetPromptResult::new(vec![
            neutral::PromptMessage::user_text(self),
        ]))
    }
}

impl IntoGetPromptResult for &str {
    fn into_get_prompt_result(self) -> McpResult<neutral::GetPromptResult> {
        Ok(neutral::GetPromptResult::new(vec![
            neutral::PromptMessage::user_text(self),
        ]))
    }
}

/// A fallible prompt render: the error *propagates*.
impl<T> IntoGetPromptResult for McpResult<T>
where
    T: IntoGetPromptResult,
{
    fn into_get_prompt_result(self) -> McpResult<neutral::GetPromptResult> {
        self.and_then(IntoGetPromptResult::into_get_prompt_result)
    }
}
