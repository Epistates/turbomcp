//! Schema generation utilities

use quote::quote;
use syn::Type;

/// Generate JSON schema for a Rust type with optional description
#[allow(dead_code)]
pub fn generate_json_schema_with_description(
    ty: &Type,
    description: Option<&str>,
) -> proc_macro2::TokenStream {
    let base_schema = generate_json_schema(ty);

    match description {
        Some(desc) => {
            quote! {
                {
                    let mut schema = #base_schema;
                    if let ::serde_json::Value::Object(ref mut map) = schema {
                        map.insert("description".to_string(), ::serde_json::Value::String(#desc.to_string()));
                    }
                    schema
                }
            }
        }
        None => base_schema,
    }
}

/// Generate JSON schema for a Rust type
#[allow(dead_code)]
pub fn generate_json_schema(ty: &Type) -> proc_macro2::TokenStream {
    // Use serde_json::Value::Object instead of json! macro to avoid expansion issues
    match ty {
        Type::Path(type_path) => {
            if let Some(segment) = type_path.path.segments.last() {
                let segment_name = segment.ident.to_string();
                match segment_name.as_str() {
                    // Handle Option<T> types
                    "Option" => {
                        if let syn::PathArguments::AngleBracketed(args) = &segment.arguments
                            && let Some(syn::GenericArgument::Type(inner_type)) = args.args.first()
                        {
                            // Generate schema for the inner type (but make it optional in the final schema)
                            return generate_json_schema(inner_type);
                        }
                        // Fallback for malformed Option
                        quote! {
                            {
                                let mut map = ::serde_json::Map::new();
                                map.insert("type".to_string(), ::serde_json::Value::String("object".to_string()));
                                ::serde_json::Value::Object(map)
                            }
                        }
                    }
                    // Handle Vec<T> types
                    "Vec" => {
                        if let syn::PathArguments::AngleBracketed(args) = &segment.arguments
                            && let Some(syn::GenericArgument::Type(inner_type)) = args.args.first()
                        {
                            let inner_schema = generate_json_schema(inner_type);
                            return quote! {
                                {
                                    let mut map = ::serde_json::Map::new();
                                    map.insert("type".to_string(), ::serde_json::Value::String("array".to_string()));
                                    map.insert("items".to_string(), #inner_schema);
                                    ::serde_json::Value::Object(map)
                                }
                            };
                        }
                        // Fallback for malformed Vec
                        quote! {
                            {
                                let mut map = ::serde_json::Map::new();
                                map.insert("type".to_string(), ::serde_json::Value::String("array".to_string()));
                                map.insert("items".to_string(), ::serde_json::Value::Object(::serde_json::Map::new()));
                                ::serde_json::Value::Object(map)
                            }
                        }
                    }
                    "i32" | "i64" | "isize" => quote! {
                        {
                            let mut map = ::serde_json::Map::new();
                            map.insert("type".to_string(), ::serde_json::Value::String("integer".to_string()));
                            ::serde_json::Value::Object(map)
                        }
                    },
                    "u32" | "u64" | "usize" => quote! {
                        {
                            let mut map = ::serde_json::Map::new();
                            map.insert("type".to_string(), ::serde_json::Value::String("integer".to_string()));
                            map.insert("minimum".to_string(), ::serde_json::Value::Number(0.into()));
                            ::serde_json::Value::Object(map)
                        }
                    },
                    "f32" | "f64" => quote! {
                        {
                            let mut map = ::serde_json::Map::new();
                            map.insert("type".to_string(), ::serde_json::Value::String("number".to_string()));
                            ::serde_json::Value::Object(map)
                        }
                    },
                    "String" => quote! {
                        {
                            let mut map = ::serde_json::Map::new();
                            map.insert("type".to_string(), ::serde_json::Value::String("string".to_string()));
                            ::serde_json::Value::Object(map)
                        }
                    },
                    "bool" => quote! {
                        {
                            let mut map = ::serde_json::Map::new();
                            map.insert("type".to_string(), ::serde_json::Value::String("boolean".to_string()));
                            ::serde_json::Value::Object(map)
                        }
                    },
                    _ => quote! {
                        {
                            let mut map = ::serde_json::Map::new();
                            map.insert("type".to_string(), ::serde_json::Value::String("object".to_string()));
                            ::serde_json::Value::Object(map)
                        }
                    },
                }
            } else {
                quote! {
                    {
                        let mut map = ::serde_json::Map::new();
                        map.insert("type".to_string(), ::serde_json::Value::String("object".to_string()));
                        ::serde_json::Value::Object(map)
                    }
                }
            }
        }
        _ => quote! {
            {
                let mut map = ::serde_json::Map::new();
                map.insert("type".to_string(), ::serde_json::Value::String("object".to_string()));
                ::serde_json::Value::Object(map)
            }
        },
    }
}
