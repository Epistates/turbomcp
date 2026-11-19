//! [`Icon`]: an image a client can show beside a server, tool, resource or
//! prompt.
//!
//! In core rather than in `turbomcp-protocol`'s neutral types because an
//! [`Implementation`](crate::Implementation) carries icons too, and core sits
//! below protocol. The serde shape is the wire's (`2025-11-25` onwards).

use alloc::string::String;
use alloc::vec::Vec;

/// A sized icon resource. `src` is an HTTP(S) URL or a base64 `data:` URI.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Icon {
    /// URI of the icon resource (HTTP/HTTPS or `data:`).
    pub src: String,
    /// MIME type override when the source's is missing or generic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// `"WxH"` strings (e.g. `"48x48"`) or `"any"`; empty = any size.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sizes: Vec<String>,
    /// The color scheme this icon is designed for; `None` = suits both.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<IconTheme>,
}

impl Icon {
    /// An icon with the given source URI (any size, MIME inferred).
    pub fn new(src: impl Into<String>) -> Self {
        Self {
            src: src.into(),
            mime_type: None,
            sizes: Vec::new(),
            theme: None,
        }
    }

    /// Declare the image's MIME type.
    #[must_use]
    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }

    /// Declare the sizes it's drawn for (`"48x48"`, `"any"` for scalable).
    #[must_use]
    pub fn with_sizes<I, S>(mut self, sizes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.sizes = sizes.into_iter().map(Into::into).collect();
        self
    }

    /// Declare the color scheme it's designed for.
    #[must_use]
    pub fn with_theme(mut self, theme: IconTheme) -> Self {
        self.theme = Some(theme);
        self
    }
}

/// The color scheme an [`Icon`] is designed for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum IconTheme {
    /// For light backgrounds.
    Light,
    /// For dark backgrounds.
    Dark,
}
