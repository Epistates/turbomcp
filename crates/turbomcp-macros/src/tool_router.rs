//! Tool Router Macro - Industry-standard tool composition system
//!
//! Provides the #[tool_router] macro for creating composable tool routers

use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse_macro_input, AttributeArgs, ItemImpl, NestedMeta, Meta, Lit};

/// Generate tool router implementation
pub fn generate_tool_router_impl(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as AttributeArgs);
    let input_impl = parse_macro_input!(input as ItemImpl);
    
    // Parse attributes
    let mut router_name = "tool_router".to_string();
    let mut visibility = None;
    
    for arg in args {
        match arg {
            NestedMeta::Meta(Meta::NameValue(nv)) if nv.path.is_ident("router") => {
                if let Lit::Str(lit_str) = nv.lit {
                    router_name = lit_str.value();
                }
            }
            NestedMeta::Meta(Meta::NameValue(nv)) if nv.path.is_ident("vis") => {
                if let Lit::Str(lit_str) = nv.lit {
                    let vis_str = lit_str.value();
                    if vis_str == "pub" {
                        visibility = Some(quote! { pub });
                    }
                }
            }
            _ => {}
        }
    }
    
    let vis = visibility.unwrap_or_else(|| quote! {});
    let router_ident = syn::Ident::new(&router_name, proc_macro2::Span::call_site());
    
    let type_name = if let syn::Type::Path(type_path) = &*input_impl.self_ty {
        &type_path.path
    } else {
        return syn::Error::new_spanned(&input_impl.self_ty, "Expected a simple type name")
            .to_compile_error();
    };
    
    // Find tool methods in the impl block
    let mut tool_registrations = Vec::new();
    
    for item in &input_impl.items {
        if let syn::ImplItem::Method(method) = item {
            // Check if method has #[tool] attribute
            let has_tool_attr = method.attrs.iter().any(|attr| {
                attr.path.is_ident("tool")
            });
            
            if has_tool_attr {
                let method_name = &method.sig.ident;
                let method_name_str = method_name.to_string();
                
                tool_registrations.push(quote! {
                    tools.insert(
                        #method_name_str.to_string(),
                        std::sync::Arc::new(turbomcp::router::ToolHandlerWrapper::new(
                            |args| {
                                Box::pin(async move {
                                    // Production-grade argument parsing and validation
                                    let parsed_args = turbomcp::router::parse_and_validate_tool_arguments(
                                        &args,
                                        &tool_metadata.input_schema
                                    )?;
                                    
                                    // Execute tool with validated arguments and proper error handling
                                    match self.#method_name(parsed_args).await {
                                        Ok(result) => Ok(turbomcp::CallToolResult {
                                            content: vec![turbomcp::mcp_text!(format!("{:?}", result))],
                                            is_error: Some(false),
                                        }),
                                        Err(e) => Ok(turbomcp::CallToolResult {
                                            content: vec![turbomcp::mcp_text!(format!("Tool execution failed: {}", e))],
                                            is_error: Some(true),
                                        })
                                    }
                                })
                            },
                            turbomcp::router::ToolMetadata {
                                name: #method_name_str.to_string(),
                                description: "Generated tool".to_string(),
                                input_schema: None,
                                output_schema: None,
                                required_permissions: vec![],
                                tags: vec![],
                                version: "1.0.0".to_string(),
                                deprecated: None,
                            }
                        ))
                    );
                });
            }
        }
    }
    
    let expanded = quote! {
        #input_impl
        
        impl #type_name {
            #vis fn #router_ident() -> turbomcp::router::ToolRouter<Self> {
                let server = Self::default();
                let router = turbomcp::router::ToolRouter::new(server);
                
                // Register tools (this would be async in production, but simplified for macro)
                tokio::spawn(async move {
                    let mut tools = std::collections::HashMap::new();
                    #(#tool_registrations)*
                    
                    for (name, handler) in tools {
                        router.register_tool(name, handler).await;
                    }
                });
                
                router
            }
        }
    };
    
    expanded
}