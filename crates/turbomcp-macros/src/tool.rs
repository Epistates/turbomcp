//! v3 tool macro - generates tool metadata with parameter parsing from function signature.
//!
//! # Per-Parameter Documentation
//!
//! The v3 macro system supports per-parameter documentation via the `#[description]` attribute:
//!
//! ```rust,ignore
//! #[tool]
//! async fn greet(
//!     #[description("The name of the person to greet")]
//!     name: String,
//!     #[description("Optional greeting prefix")]
//!     prefix: Option<String>,
//! ) -> String {
//!     // ...
//! }
//! ```
//!
//! This generates JSON Schema with parameter descriptions:
//!
//! ```json
//! {
//!   "type": "object",
//!   "properties": {
//!     "name": { "type": "string", "description": "The name of the person to greet" },
//!     "prefix": { "type": "string", "description": "Optional greeting prefix" }
//!   },
//!   "required": ["name"]
//! }
//! ```
//!
//! # Complex Type Support
//!
//! For complex types that implement `schemars::JsonSchema`, the macro automatically
//! uses the schemars-generated schema. This enables rich nested object schemas:
//!
//! ```rust,ignore
//! use schemars::JsonSchema;
//! use serde::Deserialize;
//!
//! #[derive(Deserialize, JsonSchema)]
//! struct SearchParams {
//!     /// The search query
//!     query: String,
//!     /// Maximum results to return
//!     limit: Option<i32>,
//! }
//!
//! #[tool]
//! async fn search(params: SearchParams) -> Vec<Result> {
//!     // schemars generates the full schema with nested documentation
//! }
//! ```

use proc_macro2::TokenStream;
use quote::quote;
use syn::{FnArg, ItemFn, Pat, PatType, Signature, Type};

/// Information about a tool handler method.
#[derive(Clone)]
pub struct ToolInfo {
    /// Tool name (from function name)
    pub name: String,
    /// Tool description (from doc comments or attribute)
    pub description: String,
    /// Function signature
    pub sig: Signature,
    /// Parameters extracted from signature
    pub parameters: Vec<ParameterInfo>,
    /// Tags for categorization (e.g., ["admin", "dangerous"])
    pub tags: Vec<String>,
    /// Version string (e.g., "2.0.0")
    pub version: Option<String>,
}

/// Information about a function parameter.
#[derive(Clone)]
pub struct ParameterInfo {
    /// Parameter name
    pub name: String,
    /// Parameter type
    pub ty: Type,
    /// Parameter description (from doc comments or #[description] attribute)
    pub description: Option<String>,
    /// Whether this is an optional parameter
    pub is_optional: bool,
}

/// Parsed attributes from the #[tool(...)] macro.
#[derive(Default)]
pub struct ToolAttrs {
    /// Tool description
    pub description: Option<String>,
    /// Tags for categorization
    pub tags: Vec<String>,
    /// Version string
    pub version: Option<String>,
}

impl ToolAttrs {
    /// Parse tool attributes from a syn::Attribute.
    ///
    /// Supports multiple formats:
    /// - `#[tool]` - no attributes
    /// - `#[tool("description")]` - just description
    /// - `#[tool(description = "desc", tags = ["a", "b"], version = "1.0")]` - full syntax
    pub fn parse(attr: &syn::Attribute) -> Result<Self, syn::Error> {
        let mut attrs = Self::default();

        // Handle empty #[tool]
        let syn::Meta::List(meta_list) = &attr.meta else {
            return Ok(attrs);
        };

        // Handle #[tool("description")] shorthand
        if let Ok(lit) = syn::parse2::<syn::LitStr>(meta_list.tokens.clone()) {
            attrs.description = Some(lit.value());
            return Ok(attrs);
        }

        // Parse #[tool(description = "...", tags = [...], version = "...")]
        let parser = syn::meta::parser(|meta| {
            if meta.path.is_ident("description") {
                let value: syn::LitStr = meta.value()?.parse()?;
                attrs.description = Some(value.value());
            } else if meta.path.is_ident("tags") {
                // Parse tags = ["a", "b", "c"]
                meta.parse_nested_meta(|nested| {
                    if let Ok(lit) = nested.value() {
                        if let Ok(s) = lit.parse::<syn::LitStr>() {
                            attrs.tags.push(s.value());
                        }
                    } else {
                        // Handle tags = ["a", "b"] format (array)
                        let content: syn::LitStr = nested.input.parse()?;
                        attrs.tags.push(content.value());
                    }
                    Ok(())
                })?;
            } else if meta.path.is_ident("version") {
                let value: syn::LitStr = meta.value()?.parse()?;
                attrs.version = Some(value.value());
            } else {
                // Unknown attribute - skip it
                let _ = meta.value();
            }
            Ok(())
        });

        // Try to parse, but if it fails with the nested parser, try an alternative
        if syn::parse::Parser::parse2(parser, meta_list.tokens.clone()).is_err() {
            // Alternative: parse comma-separated items including array literals
            attrs = Self::parse_alternative(&meta_list.tokens)?;
        }

        Ok(attrs)
    }

    /// Alternative parser for complex attribute syntax.
    fn parse_alternative(tokens: &proc_macro2::TokenStream) -> Result<Self, syn::Error> {
        let mut attrs = Self::default();
        let token_str = tokens.to_string();

        attrs.description = parse_quoted_value(&token_str, "description");
        attrs.version = parse_quoted_value(&token_str, "version");
        attrs.tags = parse_tags_array(&token_str);

        Ok(attrs)
    }
}

/// Parse a `key = "value"` pattern from token stream.
/// Fallback for complex attribute syntax when standard parsing fails.
pub fn parse_quoted_value(token_str: &str, key: &str) -> Option<String> {
    // Try to parse using syn's token stream first
    if let Ok(tokens) = syn::parse_str::<proc_macro2::TokenStream>(token_str) {
        for token in tokens {
            if let proc_macro2::TokenTree::Ident(ident) = &token
                && ident == key
            {
                // Found the key, look for = "value" pattern
                continue;
            }
        }
    }

    // Fallback to string manipulation if syn parsing doesn't help
    let key_start = token_str.find(key)?;
    let after_key = &token_str[key_start + key.len()..];
    let eq_pos = after_key.find('=')?;
    let after_eq = &after_key[eq_pos + 1..];
    let quote_start = after_eq.find('"')?;
    let after_quote = &after_eq[quote_start + 1..];
    let quote_end = after_quote.find('"')?;
    Some(after_quote[..quote_end].to_string())
}

/// Parse `tags = ["a", "b", "c"]` pattern from token stream.
/// Fallback for complex attribute syntax when standard parsing fails.
pub fn parse_tags_array(token_str: &str) -> Vec<String> {
    let Some(tags_start) = token_str.find("tags") else {
        return Vec::new();
    };
    let after_tags = &token_str[tags_start + 4..]; // "tags".len() == 4
    let Some(bracket_start) = after_tags.find('[') else {
        return Vec::new();
    };
    let after_bracket = &after_tags[bracket_start + 1..];
    let Some(bracket_end) = after_bracket.find(']') else {
        return Vec::new();
    };

    let tags_content = &after_bracket[..bracket_end];
    tags_content
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            if part.starts_with('"') && part.ends_with('"') && part.len() >= 2 {
                Some(part[1..part.len() - 1].to_string())
            } else {
                None
            }
        })
        .collect()
}

impl ToolInfo {
    /// Extract tool info from a function.
    pub fn from_fn(item: &ItemFn, attrs: ToolAttrs) -> Result<Self, syn::Error> {
        let name = item.sig.ident.to_string();

        // Get description from doc comments or attribute
        let doc_description = extract_doc_comments(&item.attrs);
        let description = attrs.description.or(doc_description).unwrap_or_default();

        // Analyze parameters
        let parameters = analyze_parameters(&item.sig)?;

        Ok(Self {
            name,
            description,
            sig: item.sig.clone(),
            parameters,
            tags: attrs.tags,
            version: attrs.version,
        })
    }
}

/// Extract doc comments from attributes.
fn extract_doc_comments(attrs: &[syn::Attribute]) -> Option<String> {
    let doc_lines: Vec<String> = attrs
        .iter()
        .filter_map(|attr| {
            if attr.path().is_ident("doc")
                && let syn::Meta::NameValue(meta) = &attr.meta
                && let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(lit_str),
                    ..
                }) = &meta.value
            {
                return Some(lit_str.value().trim().to_string());
            }
            None
        })
        .collect();

    if doc_lines.is_empty() {
        None
    } else {
        Some(doc_lines.join(" "))
    }
}

/// Analyze function parameters.
fn analyze_parameters(sig: &Signature) -> Result<Vec<ParameterInfo>, syn::Error> {
    let mut parameters = Vec::new();

    for input in &sig.inputs {
        match input {
            FnArg::Receiver(_) => {
                // Skip self parameter
                continue;
            }
            FnArg::Typed(PatType { pat, ty, attrs, .. }) => {
                if let Pat::Ident(pat_ident) = pat.as_ref() {
                    let param_name = pat_ident.ident.to_string();

                    // Skip context parameters
                    if is_context_type(ty) {
                        continue;
                    }

                    // Check for #[description("...")] attribute first, then fall back to doc comments
                    let description =
                        extract_description_attr(attrs).or_else(|| extract_doc_comments(attrs));
                    let is_optional = is_option_type(ty);

                    parameters.push(ParameterInfo {
                        name: param_name,
                        ty: (**ty).clone(),
                        description,
                        is_optional,
                    });
                }
            }
        }
    }

    Ok(parameters)
}

/// Extract description from #[description("...")] attribute.
fn extract_description_attr(attrs: &[syn::Attribute]) -> Option<String> {
    for attr in attrs {
        if attr.path().is_ident("description") {
            // Handle #[description("text")] - List style
            if let syn::Meta::List(meta_list) = &attr.meta
                && let Ok(lit) = syn::parse2::<syn::LitStr>(meta_list.tokens.clone())
            {
                return Some(lit.value());
            }
            // Handle #[description = "text"] - NameValue style
            if let syn::Meta::NameValue(meta_nv) = &attr.meta
                && let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(lit_str),
                    ..
                }) = &meta_nv.value
            {
                return Some(lit_str.value());
            }
        }
    }
    None
}

/// Check if a type is a context type.
fn is_context_type(ty: &Type) -> bool {
    if let Type::Path(type_path) = ty {
        type_path
            .path
            .segments
            .last()
            .is_some_and(|seg| seg.ident == "Context" || seg.ident == "RequestContext")
    } else {
        false
    }
}

/// Check if a type is Option<T>.
fn is_option_type(ty: &Type) -> bool {
    if let Type::Path(type_path) = ty {
        type_path
            .path
            .segments
            .last()
            .is_some_and(|seg| seg.ident == "Option")
    } else {
        false
    }
}

/// Generate JSON schema code for tool parameters.
///
/// This function generates code that produces a `ToolInputSchema` at runtime.
/// All types use schemars for consistent, accurate schema generation.
///
/// Uses `::turbomcp::__macro_support::` paths so users don't need to add
/// internal crates to their Cargo.toml.
pub fn generate_schema_code(parameters: &[ParameterInfo]) -> TokenStream {
    if parameters.is_empty() {
        return quote! {
            ::turbomcp::__macro_support::turbomcp_types::ToolInputSchema::empty()
        };
    }

    let mut prop_code = Vec::new();
    let mut required_names = Vec::new();

    for param in parameters {
        let name = &param.name;
        let ty = &param.ty;

        // Always use schemars for consistent schema generation
        // schemars 1.0: schema_for! returns Schema directly (not RootSchema with .schema field)
        let schema_code = quote! {
            {
                let schema = ::turbomcp::__macro_support::schemars::schema_for!(#ty);
                match ::turbomcp::__macro_support::serde_json::to_value(&schema) {
                    Ok(schema_value) => schema_value.as_object().cloned().unwrap_or_else(|| {
                        // Fallback: create minimal object schema if conversion fails
                        let mut m = ::turbomcp::__macro_support::serde_json::Map::new();
                        m.insert("type".to_string(), ::turbomcp::__macro_support::serde_json::Value::String("object".to_string()));
                        m
                    }),
                    Err(_) => {
                        // Error fallback: create minimal object schema
                        let mut m = ::turbomcp::__macro_support::serde_json::Map::new();
                        m.insert("type".to_string(), ::turbomcp::__macro_support::serde_json::Value::String("object".to_string()));
                        m
                    }
                }
            }
        };

        let description_code = if let Some(desc) = &param.description {
            quote! {
                prop.insert("description".to_string(), ::turbomcp::__macro_support::serde_json::Value::String(#desc.to_string()));
            }
        } else {
            quote! {}
        };

        prop_code.push(quote! {
            {
                let mut prop = #schema_code;
                #description_code
                properties.insert(#name.to_string(), ::turbomcp::__macro_support::serde_json::Value::Object(prop));
            }
        });

        if !param.is_optional {
            required_names.push(name.clone());
        }
    }

    quote! {
        {
            let mut properties = ::turbomcp::__macro_support::serde_json::Map::new();
            #(#prop_code)*

            let required: Vec<String> = vec![#(#required_names.to_string()),*];

            ::turbomcp::__macro_support::turbomcp_types::ToolInputSchema {
                schema_type: "object".to_string(),
                properties: Some(::turbomcp::__macro_support::serde_json::Value::Object(properties)),
                required: if required.is_empty() { None } else { Some(required) },
                additional_properties: Some(false),
            }
        }
    }
}

/// Maximum size for a single parameter value (1MB)
const MAX_PARAM_VALUE_SIZE: usize = 1024 * 1024;

/// Generate parameter extraction code with size validation.
///
/// This includes security checks to prevent DoS attacks via oversized parameters.
/// Uses `::turbomcp::__macro_support::` paths so users don't need to add
/// internal crates to their Cargo.toml.
pub fn generate_extraction_code(parameters: &[ParameterInfo]) -> TokenStream {
    if parameters.is_empty() {
        return quote! {};
    }

    // Add parameter count validation at the start
    let param_count = parameters.len();
    let mut extraction = quote! {
        // Validate parameter count (defense against parameter pollution)
        if args.len() > #param_count + 10 {
            return Err(::turbomcp::__macro_support::turbomcp_core::error::McpError::invalid_params(
                format!("Too many parameters: got {}, expected at most {}", args.len(), #param_count)
            ));
        }
    };

    for param in parameters {
        let name_str = &param.name;
        let name_ident = syn::Ident::new(&param.name, proc_macro2::Span::call_site());
        let ty = &param.ty;

        // Generate size check code
        let size_check = quote! {
            // Security: Validate parameter size before deserialization
            if let Some(v) = args.get(#name_str) {
                let size_estimate = v.to_string().len();
                if size_estimate > #MAX_PARAM_VALUE_SIZE {
                    return Err(::turbomcp::__macro_support::turbomcp_core::error::McpError::invalid_params(
                        format!("Parameter '{}' exceeds maximum size ({} bytes)", #name_str, size_estimate)
                    ));
                }
            }
        };

        if param.is_optional {
            extraction.extend(quote! {
                #size_check
                let #name_ident: #ty = args
                    .get(#name_str)
                    .map(|v| ::turbomcp::__macro_support::serde_json::from_value(v.clone()))
                    .transpose()
                    .map_err(|e| ::turbomcp::__macro_support::turbomcp_core::error::McpError::invalid_params(
                        format!("Invalid parameter '{}': {}", #name_str, e)
                    ))?
                    .flatten();
            });
        } else {
            extraction.extend(quote! {
                #size_check
                let #name_ident: #ty = args
                    .get(#name_str)
                    .ok_or_else(|| ::turbomcp::__macro_support::turbomcp_core::error::McpError::invalid_params(
                        format!("Missing required parameter: {}", #name_str)
                    ))
                    .and_then(|v| ::turbomcp::__macro_support::serde_json::from_value(v.clone())
                        .map_err(|e| ::turbomcp::__macro_support::turbomcp_core::error::McpError::invalid_params(
                            format!("Invalid parameter '{}': {}", #name_str, e)
                        )))?;
            });
        }
    }

    extraction
}

/// Generate call arguments.
pub fn generate_call_args(sig: &Signature) -> TokenStream {
    let mut args = Vec::new();
    let mut first = true;

    for input in &sig.inputs {
        match input {
            FnArg::Receiver(_) => continue,
            FnArg::Typed(PatType { pat, ty, .. }) => {
                if let Pat::Ident(pat_ident) = pat.as_ref() {
                    if is_context_type(ty) {
                        args.push(quote! { ctx });
                    } else {
                        let name = &pat_ident.ident;
                        args.push(quote! { #name });
                    }
                    first = false;
                }
            }
        }
    }

    let _ = first; // silence warning

    quote! { #(#args),* }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn test_extract_doc_comments() {
        let attrs: Vec<syn::Attribute> = vec![parse_quote!(#[doc = " This is a test"])];
        let doc = extract_doc_comments(&attrs);
        assert_eq!(doc, Some("This is a test".to_string()));
    }

    #[test]
    fn test_extract_description_attr_list_style() {
        // Test #[description("text")]
        let attrs: Vec<syn::Attribute> = vec![parse_quote!(#[description("The name to greet")])];
        let desc = extract_description_attr(&attrs);
        assert_eq!(desc, Some("The name to greet".to_string()));
    }

    #[test]
    fn test_extract_description_attr_name_value_style() {
        // Test #[description = "text"]
        let attrs: Vec<syn::Attribute> = vec![parse_quote!(#[description = "A value"])];
        let desc = extract_description_attr(&attrs);
        assert_eq!(desc, Some("A value".to_string()));
    }

    #[test]
    fn test_is_option_type() {
        let ty: Type = parse_quote!(Option<String>);
        assert!(is_option_type(&ty));

        let ty: Type = parse_quote!(String);
        assert!(!is_option_type(&ty));
    }

    #[test]
    fn test_is_context_type() {
        let ty: Type = parse_quote!(Context);
        assert!(is_context_type(&ty));

        let ty: Type = parse_quote!(RequestContext);
        assert!(is_context_type(&ty));

        let ty: Type = parse_quote!(String);
        assert!(!is_context_type(&ty));
    }
}
