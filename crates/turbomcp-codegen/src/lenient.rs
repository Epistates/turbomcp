//! Point the fields the protocol crate decodes leniently at its decoders
//! (`turbomcp_protocol::lenient`). typify has no per-field `deserialize_with`,
//! so this walks the generated syntax tree and adds one, keyed on the field's
//! type (and, for `ttlMs`, its wire name).
//!
//! - `f64` / `Option<f64>`: read through `serde_json::Number`. With
//!   `serde_json/arbitrary_precision` on anywhere in a build (feature
//!   unification), serde replays a decimal it buffered for an untagged enum
//!   or a `flatten` as serde_json's private number map, which a plain `f64`
//!   rejects: `annotations.priority: 0.5` failed the whole content block.
//! - `Option<serde_json::Value>`: present `null` is `Some(Null)`, not absent.
//!   `structuredContent` and `error.data` may be any JSON value, `null`
//!   included, and a proxy must re-send what it was sent.
//! - `ttlMs`: negative or float-spelled values are tolerated (caching).

use quote::ToTokens;
use syn::visit_mut::{self, VisitMut};
use syn::{Field, LitStr, parse_quote};

/// Add the decoders to every matching field in `file`; returns how many.
pub fn apply(file: &mut syn::File) -> usize {
    let mut pass = Lenient(0);
    pass.visit_file_mut(file);
    pass.0
}

struct Lenient(usize);

impl VisitMut for Lenient {
    fn visit_field_mut(&mut self, field: &mut Field) {
        if let Some(decoder) = decoder(field) {
            let path = format!("crate::lenient::{decoder}");
            // A decoder replaces serde's "absent `Option` is `None`" rule, so
            // an optional field needs `default` back.
            let optional = compact(&field.ty).starts_with("::std::option::Option<");
            field
                .attrs
                .push(if optional && !serde_has(field, "default") {
                    parse_quote!(#[serde(default, deserialize_with = #path)])
                } else {
                    parse_quote!(#[serde(deserialize_with = #path)])
                });
            self.0 += 1;
        }
        visit_mut::visit_field_mut(self, field);
    }
}

fn decoder(field: &Field) -> Option<&'static str> {
    match compact(&field.ty).as_str() {
        "f64" => Some("f64"),
        "::std::option::Option<f64>" => Some("option_f64"),
        "::std::option::Option<::serde_json::Value>" => Some("present"),
        "u64" if renamed_to(field, "ttlMs") => Some("ttl_ms"),
        _ => None,
    }
}

fn compact(ty: &syn::Type) -> String {
    ty.to_token_stream()
        .to_string()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// Whether a `#[serde(...)]` on `field` sets `key` (bare or `key = …`).
fn serde_has(field: &Field, key: &str) -> bool {
    serde_value(field, key).is_some()
}

fn renamed_to(field: &Field, name: &str) -> bool {
    serde_value(field, "rename").flatten().as_deref() == Some(name)
}

/// `Some(Some(v))` for `key = "v"`, `Some(None)` for a bare `key`.
fn serde_value(field: &Field, key: &str) -> Option<Option<String>> {
    let mut found = None;
    for attr in field.attrs.iter().filter(|a| a.path().is_ident("serde")) {
        let _ = attr.parse_nested_meta(|meta| {
            let value = if meta.input.peek(syn::Token![=]) {
                let expr: syn::Expr = meta.value()?.parse()?;
                match expr {
                    syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(s),
                        ..
                    }) => Some(LitStr::value(&s)),
                    _ => None,
                }
            } else {
                None
            };
            if meta.path.is_ident(key) {
                found = Some(value);
            }
            Ok(())
        });
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn applied(src: &str) -> String {
        let mut file: syn::File = syn::parse_str(src).unwrap();
        apply(&mut file);
        prettyplease::unparse(&file)
    }

    #[test]
    fn floats_and_any_values_get_their_decoders() {
        let out = applied(
            r#"
            pub struct A {
                pub progress: f64,
                #[serde(skip_serializing_if = "::std::option::Option::is_none")]
                pub total: ::std::option::Option<f64>,
                #[serde(rename = "structuredContent", skip_serializing_if = "::std::option::Option::is_none")]
                pub structured: ::std::option::Option<::serde_json::Value>,
                #[serde(rename = "ttlMs", default)]
                pub ttl_ms: u64,
                pub count: u64,
            }
            pub enum V { Number(f64), Integer(i64) }
            "#,
        );
        assert!(out.contains(
            "#[serde(deserialize_with = \"crate::lenient::f64\")]\n    pub progress: f64"
        ));
        assert!(
            out.contains("#[serde(default, deserialize_with = \"crate::lenient::option_f64\")]")
        );
        assert!(out.contains("#[serde(default, deserialize_with = \"crate::lenient::present\")]"));
        // `ttlMs` already has `default`; a second one would be a serde error.
        assert!(out.contains(
            "#[serde(rename = \"ttlMs\", default)]\n    #[serde(deserialize_with = \"crate::lenient::ttl_ms\")]"
        ));
        assert!(out.contains("Number(#[serde(deserialize_with = \"crate::lenient::f64\")] f64)"));
        assert_eq!(out.matches("deserialize_with").count(), 5);
    }
}
