//! World-class attribute parsing for TurboMCP macros
//!
//! This module provides robust, syn-based parsing for macro attributes,
//! following patterns from Serde, Clap, and other world-class Rust libraries.

use quote::quote;
use syn::{
    Expr, Ident, Lit, Token,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
};

/// A single root declaration in the server macro
#[derive(Debug, Clone)]
pub struct Root {
    pub uri: String,
    pub name: Option<String>,
}

impl Root {
    /// Parse from "uri:name" or just "uri" format
    /// Handles file:// URIs correctly by finding the last colon for the name separator
    pub fn from_str(s: &str) -> Self {
        // For file URIs, we need to be careful not to split on the protocol colon
        // Look for the last colon that could be a separator
        if s.starts_with("file://") || s.starts_with("http://") || s.starts_with("https://") {
            // Find the last colon in the string
            if let Some(last_colon) = s.rfind(':') {
                // Check if this colon is part of the protocol
                let before_colon = &s[..last_colon];
                if before_colon == "file"
                    || before_colon == "http"
                    || before_colon == "https"
                    || before_colon.ends_with("//")
                {
                    // This is the protocol colon, no name specified
                    Root {
                        uri: s.to_string(),
                        name: None,
                    }
                } else {
                    // This colon separates the URI from the name
                    Root {
                        uri: before_colon.to_string(),
                        name: Some(s[last_colon + 1..].to_string()),
                    }
                }
            } else {
                Root {
                    uri: s.to_string(),
                    name: None,
                }
            }
        } else {
            // For non-URI strings, use simple colon splitting
            if let Some(colon_pos) = s.find(':') {
                Root {
                    uri: s[..colon_pos].to_string(),
                    name: Some(s[colon_pos + 1..].to_string()),
                }
            } else {
                Root {
                    uri: s.to_string(),
                    name: None,
                }
            }
        }
    }
}

/// Server macro attributes with world-class parsing
#[derive(Debug, Default)]
pub struct ServerAttrs {
    pub name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub roots: Vec<Root>,
}

impl ServerAttrs {
    /// Parse from the macro attribute arguments
    /// Supports multiple syntaxes for maximum ergonomics:
    /// - name = "server-name"
    /// - version = "1.0.0"
    /// - description = "Server description"
    /// - root = "/path:Name"
    /// - root = "/another/path"
    pub fn from_args(args: proc_macro::TokenStream) -> syn::Result<Self> {
        let mut attrs = ServerAttrs::default();

        if args.is_empty() {
            return Ok(attrs);
        }

        // Parse as attribute arguments
        let parsed = syn::parse::<ServerAttrArgs>(args)?;

        for item in parsed.items {
            match item.name.to_string().as_str() {
                "name" => {
                    if let Some(value) = item.get_string_value() {
                        attrs.name = Some(value);
                    }
                }
                "version" => {
                    if let Some(value) = item.get_string_value() {
                        attrs.version = Some(value);
                    }
                }
                "description" => {
                    if let Some(value) = item.get_string_value() {
                        attrs.description = Some(value);
                    }
                }
                "root" => {
                    if let Some(value) = item.get_string_value() {
                        attrs.roots.push(Root::from_str(&value));
                    }
                }
                _ => {
                    // Ignore unknown attributes for forward compatibility
                }
            }
        }

        Ok(attrs)
    }

    /// Generate the roots configuration code for the server builder
    pub fn generate_roots_config(&self) -> proc_macro2::TokenStream {
        if self.roots.is_empty() {
            return quote! {};
        }

        let root_configs: Vec<_> = self
            .roots
            .iter()
            .map(|root| {
                let uri = &root.uri;
                match &root.name {
                    Some(name) => quote! {
                        builder = builder.root(#uri, Some(#name.to_string()));
                    },
                    None => quote! {
                        builder = builder.root(#uri, None);
                    },
                }
            })
            .collect();

        quote! {
            #(#root_configs)*
        }
    }
}

/// A single attribute item (name = value)
struct AttrItem {
    name: Ident,
    _eq: Token![=],
    value: Expr,
}

impl AttrItem {
    /// Get the string value if this is a string literal
    fn get_string_value(&self) -> Option<String> {
        match &self.value {
            Expr::Lit(lit) => match &lit.lit {
                Lit::Str(s) => Some(s.value()),
                _ => None,
            },
            _ => None,
        }
    }
}

impl Parse for AttrItem {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(AttrItem {
            name: input.parse()?,
            _eq: input.parse()?,
            value: input.parse()?,
        })
    }
}

/// Collection of attribute items
struct ServerAttrArgs {
    items: Vec<AttrItem>,
}

impl Parse for ServerAttrArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let items = Punctuated::<AttrItem, Token![,]>::parse_terminated(input)?
            .into_iter()
            .collect();
        Ok(ServerAttrArgs { items })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_root_parsing() {
        let root1 = Root::from_str("/path/to/dir:My Directory");
        assert_eq!(root1.uri, "/path/to/dir");
        assert_eq!(root1.name, Some("My Directory".to_string()));

        let root2 = Root::from_str("/tmp");
        assert_eq!(root2.uri, "/tmp");
        assert_eq!(root2.name, None);
    }
}
