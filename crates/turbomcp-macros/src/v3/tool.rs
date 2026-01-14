//! v3 tool macro - generates tool metadata with parameter parsing from function signature.

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
}

/// Information about a function parameter.
#[derive(Clone)]
pub struct ParameterInfo {
    /// Parameter name
    pub name: String,
    /// Parameter type
    pub ty: Type,
    /// Parameter description (from doc comments)
    pub description: Option<String>,
    /// Whether this is an optional parameter
    pub is_optional: bool,
}

impl ToolInfo {
    /// Extract tool info from a function.
    pub fn from_fn(item: &ItemFn, attr_description: Option<String>) -> Result<Self, syn::Error> {
        let name = item.sig.ident.to_string();

        // Get description from doc comments or attribute
        let doc_description = extract_doc_comments(&item.attrs);
        let description = attr_description.or(doc_description).unwrap_or_default();

        // Analyze parameters
        let parameters = analyze_parameters(&item.sig)?;

        Ok(Self {
            name,
            description,
            sig: item.sig.clone(),
            parameters,
        })
    }
}

/// Extract doc comments from attributes.
fn extract_doc_comments(attrs: &[syn::Attribute]) -> Option<String> {
    let doc_lines: Vec<String> = attrs
        .iter()
        .filter_map(|attr| {
            if attr.path().is_ident("doc") {
                if let syn::Meta::NameValue(meta) = &attr.meta {
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(lit_str),
                        ..
                    }) = &meta.value
                    {
                        return Some(lit_str.value().trim().to_string());
                    }
                }
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

                    let description = extract_doc_comments(attrs);
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
pub fn generate_schema_code(parameters: &[ParameterInfo]) -> TokenStream {
    if parameters.is_empty() {
        return quote! {
            ::turbomcp_types::ToolInputSchema::empty()
        };
    }

    let mut prop_code = Vec::new();
    let mut required_names = Vec::new();

    for param in parameters {
        let name = &param.name;
        let schema = generate_type_schema(&param.ty);

        let description_code = if let Some(desc) = &param.description {
            quote! {
                prop.insert("description".to_string(), ::serde_json::Value::String(#desc.to_string()));
            }
        } else {
            quote! {}
        };

        prop_code.push(quote! {
            {
                let mut prop = #schema;
                #description_code
                properties.insert(#name.to_string(), ::serde_json::Value::Object(prop));
            }
        });

        if !param.is_optional {
            required_names.push(name.clone());
        }
    }

    quote! {
        {
            let mut properties = ::serde_json::Map::new();
            #(#prop_code)*

            let required: Vec<String> = vec![#(#required_names.to_string()),*];

            ::turbomcp_types::ToolInputSchema {
                schema_type: "object".to_string(),
                properties: Some(::serde_json::Value::Object(properties)),
                required: if required.is_empty() { None } else { Some(required) },
                additional_properties: Some(false),
            }
        }
    }
}

/// Generate JSON schema for a Rust type.
fn generate_type_schema(ty: &Type) -> TokenStream {
    let type_name = type_to_json_schema_type(ty);

    match type_name.as_str() {
        "string" => quote! {
            {
                let mut m = ::serde_json::Map::new();
                m.insert("type".to_string(), ::serde_json::Value::String("string".to_string()));
                m
            }
        },
        "integer" => quote! {
            {
                let mut m = ::serde_json::Map::new();
                m.insert("type".to_string(), ::serde_json::Value::String("integer".to_string()));
                m
            }
        },
        "number" => quote! {
            {
                let mut m = ::serde_json::Map::new();
                m.insert("type".to_string(), ::serde_json::Value::String("number".to_string()));
                m
            }
        },
        "boolean" => quote! {
            {
                let mut m = ::serde_json::Map::new();
                m.insert("type".to_string(), ::serde_json::Value::String("boolean".to_string()));
                m
            }
        },
        "array" => quote! {
            {
                let mut m = ::serde_json::Map::new();
                m.insert("type".to_string(), ::serde_json::Value::String("array".to_string()));
                m
            }
        },
        "object" => quote! {
            {
                let mut m = ::serde_json::Map::new();
                m.insert("type".to_string(), ::serde_json::Value::String("object".to_string()));
                m
            }
        },
        _ => quote! {
            {
                let mut m = ::serde_json::Map::new();
                m.insert("type".to_string(), ::serde_json::Value::String("string".to_string()));
                m
            }
        },
    }
}

/// Convert a Rust type to JSON Schema type.
fn type_to_json_schema_type(ty: &Type) -> String {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            let ident = segment.ident.to_string();
            return match ident.as_str() {
                "String" | "str" => "string".to_string(),
                "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64"
                | "u128" | "usize" => "integer".to_string(),
                "f32" | "f64" => "number".to_string(),
                "bool" => "boolean".to_string(),
                "Vec" => "array".to_string(),
                "HashMap" | "BTreeMap" | "Map" => "object".to_string(),
                "Option" => {
                    // Extract inner type for Option
                    if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                        if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                            return type_to_json_schema_type(inner);
                        }
                    }
                    "string".to_string()
                }
                _ => "object".to_string(), // Default to object for complex types
            };
        }
    }
    "string".to_string()
}

/// Generate parameter extraction code.
pub fn generate_extraction_code(parameters: &[ParameterInfo]) -> TokenStream {
    if parameters.is_empty() {
        return quote! {};
    }

    let mut extraction = quote! {};

    for param in parameters {
        let name_str = &param.name;
        let name_ident = syn::Ident::new(&param.name, proc_macro2::Span::call_site());
        let ty = &param.ty;

        if param.is_optional {
            extraction.extend(quote! {
                let #name_ident: #ty = args
                    .get(#name_str)
                    .map(|v| ::serde_json::from_value(v.clone()))
                    .transpose()
                    .map_err(|e| ::turbomcp_types::McpError::invalid_params(
                        format!("Invalid parameter '{}': {}", #name_str, e)
                    ))?
                    .flatten();
            });
        } else {
            extraction.extend(quote! {
                let #name_ident: #ty = args
                    .get(#name_str)
                    .ok_or_else(|| ::turbomcp_types::McpError::invalid_params(
                        format!("Missing required parameter: {}", #name_str)
                    ))
                    .and_then(|v| ::serde_json::from_value(v.clone())
                        .map_err(|e| ::turbomcp_types::McpError::invalid_params(
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

    #[test]
    fn test_type_to_json_schema_type() {
        assert_eq!(type_to_json_schema_type(&parse_quote!(String)), "string");
        assert_eq!(type_to_json_schema_type(&parse_quote!(i64)), "integer");
        assert_eq!(type_to_json_schema_type(&parse_quote!(f64)), "number");
        assert_eq!(type_to_json_schema_type(&parse_quote!(bool)), "boolean");
        assert_eq!(
            type_to_json_schema_type(&parse_quote!(Vec<String>)),
            "array"
        );
    }
}
