//! Return-type conversions for macro-generated handlers.
//!
//! A `#[tool]` / `#[resource]` / `#[prompt]` method may return a plain `String`,
//! the neutral result type directly, or a fallible `McpResult<_>`. These traits
//! let the `#[server]` macro emit one uniform `.into_*()` call regardless, while
//! enforcing the spec's differing error semantics per method family:
//!
//! - **Tools**: a handler error becomes a `CallToolResult { is_error: true }`
//!   (the model sees it and self-corrects), *not* a JSON-RPC error.
//! - **Resources / prompts**: a handler error *propagates* as a JSON-RPC error,
//!   since `resources/read` and `prompts/get` have no `is_error` channel.

use serde::Serialize;
use turbomcp_core::{McpError, McpResult};
use turbomcp_protocol::neutral;

/// Convert a `#[tool]` return value into a [`neutral::CallToolResult`].
pub trait IntoCallToolResult {
    /// Perform the conversion.
    ///
    /// # Errors
    /// A handler's error, passed on, or a value that can't be rendered.
    fn into_call_tool_result(self) -> McpResult<neutral::CallToolResult>;

    /// The conversion for a call made in `ctx`, which is what `#[server]`
    /// calls: the same, except that an internal error's text is masked when
    /// the server was built with `mask_internal_errors`. Implement
    /// [`into_call_tool_result`](Self::into_call_tool_result) only.
    ///
    /// # Errors
    /// As [`into_call_tool_result`](Self::into_call_tool_result).
    fn into_call_tool_result_for(
        self,
        ctx: &crate::CallToolContext,
    ) -> McpResult<neutral::CallToolResult>
    where
        Self: Sized,
    {
        let _ = ctx;
        self.into_call_tool_result()
    }
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

/// A handler's whole input as one type: `T`'s JSON Schema is the tool's
/// `inputSchema` (or, on a `#[prompt]`, its properties are the prompt's
/// arguments), and the arguments deserialize into it. For an argument type
/// shared by several tools, or one that needs `#[serde(default)]`,
/// `deny_unknown_fields` or field-level `#[schemars(…)]`.
///
/// ```ignore
/// #[derive(serde::Deserialize, schemars::JsonSchema)]
/// struct Search { query: String, #[serde(default)] limit: Option<u32> }
///
/// #[tool]
/// async fn search(&self, Params(search): Params<Search>) -> String { search.query }
/// ```
///
/// A `Params` is the handler's only input besides its context. A prompt's
/// arguments are strings on the wire, so a `Params` for a prompt deserializes
/// from string values.
#[derive(Clone, Copy, Debug, Default)]
pub struct Params<T>(pub T);

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

/// Return an image from a `#[tool]`: base64-encoded `data` plus its
/// `mime_type` (e.g. `image/png`) become a single image content block, with
/// any annotations and `_meta` you attach.
///
/// ```ignore
/// #[tool(description = "Render a chart")]
/// async fn chart(&self) -> Image {
///     Image::new(png_base64, "image/png").with_meta_entry("com.example/dpi", json!(144))
/// }
/// ```
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Image {
    /// Base64-encoded image bytes.
    pub data: String,
    /// The image MIME type.
    pub mime_type: String,
    /// Display annotations (audience, priority, last modified).
    pub annotations: Option<neutral::Annotations>,
    /// The block's `_meta`.
    pub meta: serde_json::Map<String, serde_json::Value>,
}

/// Return audio from a `#[tool]`: base64-encoded `data` plus its
/// `mime_type` (e.g. `audio/wav`) become a single audio content block, with
/// any annotations and `_meta` you attach.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Audio {
    /// Base64-encoded audio bytes.
    pub data: String,
    /// The audio MIME type.
    pub mime_type: String,
    /// Display annotations (audience, priority, last modified).
    pub annotations: Option<neutral::Annotations>,
    /// The block's `_meta`.
    pub meta: serde_json::Map<String, serde_json::Value>,
}

macro_rules! media {
    ($ty:ident, $what:literal, $content:ident) => {
        impl $ty {
            #[doc = concat!("A", $what, " block of base64 `data` with its `mime_type`.")]
            #[must_use]
            pub fn new(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
                Self {
                    data: data.into(),
                    mime_type: mime_type.into(),
                    annotations: None,
                    meta: serde_json::Map::new(),
                }
            }

            /// Set its display annotations.
            #[must_use]
            pub fn with_annotations(mut self, annotations: neutral::Annotations) -> Self {
                self.annotations = Some(annotations);
                self
            }

            /// Add one `_meta` entry.
            #[must_use]
            pub fn with_meta_entry(
                mut self,
                key: impl Into<String>,
                value: serde_json::Value,
            ) -> Self {
                self.meta.insert(key.into(), value);
                self
            }
        }

        impl IntoCallToolResult for $ty {
            fn into_call_tool_result(self) -> McpResult<neutral::CallToolResult> {
                let mut block = neutral::Content::$content(self.data, self.mime_type);
                if let Some(annotations) = self.annotations {
                    block = block.with_annotations(annotations);
                }
                for (key, value) in self.meta {
                    block = block.with_meta_entry(key, value);
                }
                Ok(neutral::CallToolResult::new(vec![block]))
            }
        }
    };
}
media!(Image, "n image", image);
media!(Audio, "n audio", audio);

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
/// Four errors keep propagating instead, because they say the call could not
/// be made rather than that the tool ran and failed: the MRTR abort sentinel
/// (the dispatcher answers an `InputRequiredResult`), a missing client
/// capability (SEP-2575 requires `MissingRequiredClientCapabilityError`,
/// which a `CallToolResult` cannot express — a client that saw `isError`
/// would have no way to learn it needs to re-declare and retry), insufficient
/// scope, which the router turns into a tool error *and* a step-up challenge
/// for the HTTP transport, and [`McpError::Rpc`], the explicit way to answer
/// with a protocol error (`McpError::url_elicitation_required`, say).
impl<T> IntoCallToolResult for McpResult<T>
where
    T: IntoCallToolResult,
{
    fn into_call_tool_result(self) -> McpResult<neutral::CallToolResult> {
        match self {
            Ok(v) => v.into_call_tool_result(),
            Err(
                e @ (McpError::InputRequired
                | McpError::MissingRequiredCapability(_)
                | McpError::InsufficientScope(_)
                | McpError::Rpc { .. }),
            ) => Err(e),
            Err(e) => Ok(neutral::CallToolResult::error(e.to_string())),
        }
    }

    fn into_call_tool_result_for(
        self,
        ctx: &crate::CallToolContext,
    ) -> McpResult<neutral::CallToolResult> {
        match self {
            Ok(v) => v.into_call_tool_result_for(ctx),
            Err(e @ McpError::Internal(_)) if crate::masking::enabled(&ctx.base.extensions) => Ok(
                neutral::CallToolResult::error(crate::masking::masked(&e.to_string())),
            ),
            other => other.into_call_tool_result(),
        }
    }
}

/// Convert a `#[resource]` return value into a [`neutral::ReadResourceResult`],
/// using the resolved `uri` when wrapping bare text.
pub trait IntoReadResourceResult {
    /// Perform the conversion. `uri` is the resource being read.
    ///
    /// # Errors
    /// A handler's error, passed on, or a value that can't be rendered.
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
    ///
    /// # Errors
    /// A handler's error, passed on, or a value that can't be rendered.
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
