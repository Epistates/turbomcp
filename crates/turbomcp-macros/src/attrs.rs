//! The `key = value` grammar shared by `#[tool]`, `#[resource]`, and `#[prompt]`.
//!
//! Each marker used to parse its own arguments. `#[tool]` had a strict parser
//! whose errors were then discarded by a string-scanning fallback, and
//! `#[resource]`/`#[prompt]` had only the fallback — so on all three a typo
//! like `descriptio = "..."` compiled into a handler with no description and no
//! diagnostic. One strict grammar now serves all three: the keys every marker
//! shares are handled here, each marker adds its own, and anything else is a
//! compile error naming every key the marker accepts.

use proc_macro2::TokenStream;
use syn::meta::ParseNestedMeta;

/// Keys every handler marker accepts.
const COMMON_KEYS: &[&str] = &["description", "tags", "version", "title", "icons"];

/// Metadata every handler marker accepts.
#[derive(Default)]
pub struct CommonAttrs {
    /// `description = "..."`. Takes precedence over the doc comment.
    pub description: Option<String>,
    /// `tags = ["a", "b"]`, surfaced in `_meta`.
    pub tags: Vec<String>,
    /// `version = "..."`, surfaced in `_meta`.
    pub version: Option<String>,
    /// `title = "..."` (SEP-973).
    pub title: Option<String>,
    /// `icons = ["https://…"]` (SEP-973). Each entry becomes an `Icon { src }`;
    /// richer icons (mimeType, sizes, theme) are reachable through the runtime
    /// builder.
    pub icons: Vec<String>,
}

/// Parse the `key = value, ...` arguments of the `marker` attribute.
///
/// The shared keys land in the returned [`CommonAttrs`]. Every other key is
/// offered to `extra`, which returns `Ok(true)` if it consumed it; `extra_keys`
/// names those keys for the diagnostic an unrecognised one produces.
pub fn parse_marker_attrs(
    tokens: TokenStream,
    marker: &str,
    extra_keys: &[&str],
    mut extra: impl FnMut(&ParseNestedMeta<'_>) -> syn::Result<bool>,
) -> syn::Result<CommonAttrs> {
    let mut common = CommonAttrs::default();

    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("description") {
            common.description = Some(parse_lit_str(&meta)?);
        } else if meta.path.is_ident("tags") {
            common.tags = parse_lit_str_array(&meta)?;
        } else if meta.path.is_ident("version") {
            common.version = Some(parse_lit_str(&meta)?);
        } else if meta.path.is_ident("title") {
            common.title = Some(parse_lit_str(&meta)?);
        } else if meta.path.is_ident("icons") {
            common.icons = parse_lit_str_array(&meta)?;
        } else if !extra(&meta)? {
            let key = meta
                .path
                .get_ident()
                .map(|i| i.to_string())
                .unwrap_or_else(|| "<unknown>".to_string());
            let expected = COMMON_KEYS
                .iter()
                .chain(extra_keys)
                .map(|k| format!("`{k}`"))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(meta.error(format!(
                "unknown #[{marker}] attribute key `{key}`; expected one of {expected}"
            )));
        }
        Ok(())
    });

    syn::parse::Parser::parse2(parser, tokens)?;
    Ok(common)
}

/// Parse `key = "value"`.
pub fn parse_lit_str(meta: &ParseNestedMeta<'_>) -> syn::Result<String> {
    Ok(meta.value()?.parse::<syn::LitStr>()?.value())
}

/// Parse `key = ["a", "b", ...]`.
pub fn parse_lit_str_array(meta: &ParseNestedMeta<'_>) -> syn::Result<Vec<String>> {
    let value = meta.value()?;
    let arr;
    syn::bracketed!(arr in value);
    let parsed: syn::punctuated::Punctuated<syn::LitStr, syn::Token![,]> =
        syn::punctuated::Punctuated::parse_terminated(&arr)?;
    Ok(parsed.into_iter().map(|s| s.value()).collect())
}

/// Parse `key = true|false`.
pub fn parse_lit_bool(meta: &ParseNestedMeta<'_>) -> syn::Result<bool> {
    Ok(meta.value()?.parse::<syn::LitBool>()?.value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    #[test]
    fn unknown_keys_name_every_accepted_key() {
        let err = parse_marker_attrs(
            quote!(descriptio = "typo"),
            "prompt",
            &["mime_type"],
            |_| Ok(false),
        )
        .err()
        .expect("a misspelled key must not compile");
        let message = err.to_string();
        assert!(message.contains("#[prompt]"), "{message}");
        assert!(message.contains("`descriptio`"), "{message}");
        for key in COMMON_KEYS.iter().chain(&["mime_type"]) {
            assert!(message.contains(&format!("`{key}`")), "{message}");
        }
    }

    #[test]
    fn shared_keys_parse_and_extra_keys_reach_the_marker() {
        let mut mime = None;
        let common = parse_marker_attrs(
            quote!(
                description = "d",
                tags = ["a", "b"],
                version = "2",
                title = "T",
                icons = ["https://x/i.png"],
                mime_type = "text/csv"
            ),
            "resource",
            &["mime_type"],
            |meta| {
                if meta.path.is_ident("mime_type") {
                    mime = Some(parse_lit_str(meta)?);
                    return Ok(true);
                }
                Ok(false)
            },
        )
        .unwrap();
        assert_eq!(common.description.as_deref(), Some("d"));
        assert_eq!(common.tags, ["a", "b"]);
        assert_eq!(common.version.as_deref(), Some("2"));
        assert_eq!(common.title.as_deref(), Some("T"));
        assert_eq!(common.icons, ["https://x/i.png"]);
        assert_eq!(mime.as_deref(), Some("text/csv"));
    }

    #[test]
    fn a_malformed_value_is_an_error_not_a_default() {
        // The old fallback parser turned this into "no description".
        assert!(parse_marker_attrs(quote!(description = 42), "tool", &[], |_| Ok(false)).is_err());
    }
}
