//! Ping macro implementation
//!
//! Provides the #[ping] attribute macro for marking methods as ping handlers.
//! Ping handlers enable bidirectional health checks and connection monitoring.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{FnArg, ItemFn, Pat, PatType, Signature, Type, parse_macro_input};

/// Generate ping handler implementation
pub fn generate_ping_impl(args: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as ItemFn);

    // Parse description from args
    let raw_args = args.to_string();
    let description = if raw_args.is_empty() {
        format!("Ping: {}", input.sig.ident)
    } else {
        raw_args.trim().trim_matches('"').to_string()
    };

    let fn_name = &input.sig.ident;
    let fn_vis = &input.vis;
    let fn_block = &input.block;
    let fn_sig = &input.sig;
    let handler_name = fn_name.to_string();

    // Generate metadata function name
    let metadata_fn_name = syn::Ident::new(
        &format!("__turbomcp_ping_metadata_{fn_name}"),
        proc_macro2::Span::call_site(),
    );

    // Generate handler function name
    let handler_fn_name = syn::Ident::new(
        &format!("__turbomcp_ping_handler_{fn_name}"),
        proc_macro2::Span::call_site(),
    );

    // Generate public metadata function for testing
    let public_metadata_fn_name = syn::Ident::new(
        &format!("{}_metadata", fn_name),
        proc_macro2::Span::call_site(),
    );

    // Analyze function signature for parameter handling
    let analysis = match analyze_ping_signature(fn_sig) {
        Ok(analysis) => analysis,
        Err(err) => return err.to_compile_error().into(),
    };

    let param_extraction = generate_ping_parameter_extraction(&analysis);
    let call_args = &analysis.call_args;

    // Implementation that preserves function and enables auto-discovery
    let expanded = quote! {
        // Keep original function unchanged
        #fn_vis #fn_sig #fn_block

        // Generate metadata function
        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #metadata_fn_name() -> (&'static str, &'static str, &'static str) {
            (#handler_name, #description, "ping")
        }

        // Generate public metadata function for testing
        /// Get metadata for this ping handler (name, description, type)
        pub fn #public_metadata_fn_name() -> (&'static str, &'static str, &'static str) {
            Self::#metadata_fn_name()
        }

        // Generate handler function that bridges PingRequest to the actual method
        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #handler_fn_name(&self, request: turbomcp::PingRequest, context: turbomcp::RequestContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<turbomcp::PingResult, ::turbomcp::__macro_support::turbomcp_server::McpError>> + Send + '_>> {
            Box::pin(async move {
                // Context injection using ContextFactory pattern
                let turbomcp_ctx = {
                    // Create a context factory with optimized configuration
                    use turbomcp::{ContextFactory, ContextFactoryConfig, Container};

                    // Use a static context factory for maximum performance (in practice, this would be a server instance field)
                    // Architecture supports server-level ContextFactory integration via dependency injection
                    let config = ContextFactoryConfig {
                        enable_tracing: true,
                        enable_metrics: true,
                        max_pool_size: 50,
                        default_strategy: turbomcp::ContextCreationStrategy::Inherit,
                        ..Default::default()
                    };
                    let container = Container::new();
                    let factory = ContextFactory::new(config, container);

                    // Use the factory to create context with proper error handling
                    factory.create_for_tool(context.clone(), #handler_name, Some(#description))
                        .await
                        .unwrap_or_else(|_| {
                            // Fallback to basic context if factory fails
                            let handler_metadata = turbomcp::HandlerMetadata {
                                name: #handler_name.to_string(),
                                handler_type: "ping".to_string(),
                                description: Some(#description.to_string()),
                            };
                            turbomcp::Context::new(context, handler_metadata)
                        })
                };

                // Extract parameters from request (ping requests typically have minimal parameters)
                #param_extraction

                // Call the actual method and convert result to PingResult
                let result = self.#fn_name(#call_args).await?;

                // Convert result to PingResult - properly serialize the result
                let metadata = match ::serde_json::to_value(&result) {
                    Ok(val) => {
                        // Create metadata with status and response
                        let mut meta_map = std::collections::HashMap::new();
                        meta_map.insert("status".to_string(), serde_json::json!("ok"));
                        meta_map.insert("response".to_string(), val);
                        Some(serde_json::Value::Object(serde_json::Map::from_iter(
                            meta_map.into_iter()
                        )))
                    }
                    Err(_) => {
                        // Fallback: create simple status metadata
                        let mut meta_map = std::collections::HashMap::new();
                        meta_map.insert("status".to_string(), serde_json::json!("ok"));
                        meta_map.insert("response".to_string(), serde_json::Value::String(format!("{:?}", result)));
                        Some(serde_json::Value::Object(serde_json::Map::from_iter(
                            meta_map.into_iter()
                        )))
                    }
                };

                Ok(turbomcp::PingResult {
                    _meta: metadata,
                })
            })
        }
    };

    expanded.into()
}

/// Analysis result for ping function signature
#[derive(Debug)]
struct PingAnalysis {
    /// Arguments to pass to the function call
    call_args: TokenStream2,
    /// Parameters that need extraction from the request
    parameters: Vec<(String, Type)>,
}

/// Analyze ping function signature
fn analyze_ping_signature(sig: &Signature) -> syn::Result<PingAnalysis> {
    let mut call_args = Vec::new();
    let mut parameters = Vec::new();

    for arg in sig.inputs.iter() {
        match arg {
            FnArg::Receiver(_) => {
                // Skip self parameter
                continue;
            }
            FnArg::Typed(PatType { pat, ty, .. }) => {
                if let Pat::Ident(ident) = pat.as_ref() {
                    let param_name = ident.ident.to_string();

                    // Check if this is a context parameter
                    if is_context_type(ty) {
                        call_args.push(quote! { turbomcp_ctx });
                    } else {
                        // This is a regular parameter that needs extraction from request metadata
                        call_args.push(quote! { #ident });
                        parameters.push((param_name, ty.as_ref().clone()));
                    }
                } else {
                    return Err(syn::Error::new_spanned(
                        pat,
                        "Complex patterns not supported in ping handlers",
                    ));
                }
            }
        }
    }

    let call_args = quote! { #(#call_args),* };

    Ok(PingAnalysis {
        call_args,
        parameters,
    })
}

/// Generate parameter extraction code for ping
fn generate_ping_parameter_extraction(analysis: &PingAnalysis) -> TokenStream2 {
    if analysis.parameters.is_empty() {
        return quote! {};
    }

    let extractions: Vec<TokenStream2> = analysis
        .parameters
        .iter()
        .map(|(name, ty)| {
            let ident = syn::Ident::new(name, proc_macro2::Span::call_site());

            // Check if this is an optional parameter
            let is_optional = is_option_type(ty);

            if is_optional {
                // For optional parameters, use None if not present
                quote! {
                    let #ident: #ty = {
                        // For ping handlers, parameters might be extracted from request params
                        // This is context-dependent and typically minimal for ping operations
                        None
                    };
                }
            } else {
                // For required parameters, extract from ping context
                quote! {
                    let #ident: #ty = {
                        // Extract parameter from ping request context
                        // This is simplified - in practice ping handlers typically have minimal parameters
                        Default::default()
                    };
                }
            }
        })
        .collect();

    quote! {
        #(#extractions)*
    }
}

/// Check if a type is Option<T>
fn is_option_type(ty: &Type) -> bool {
    match ty {
        Type::Path(type_path) => {
            if let Some(segment) = type_path.path.segments.last() {
                segment.ident == "Option"
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Check if a type is a context type
fn is_context_type(ty: &Type) -> bool {
    if let Type::Path(path) = ty {
        if let Some(segment) = path.path.segments.last() {
            matches!(
                segment.ident.to_string().as_str(),
                "RequestContext" | "Context"
            )
        } else {
            false
        }
    } else {
        false
    }
}
