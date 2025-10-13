//! Resource template macro implementation
//!
//! Provides the #[template] attribute macro for marking methods as resource template handlers.
//! Templates enable parameterized resource URIs using RFC 6570 URI templates.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{FnArg, ItemFn, Pat, PatType, Signature, Type, parse_macro_input};

/// Generate resource template handler implementation
pub fn generate_template_impl(args: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as ItemFn);

    // Parse URI template from args
    let raw_args = args.to_string();
    let uri_template = if raw_args.is_empty() {
        format!("template://{}", input.sig.ident)
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
        &format!("__turbomcp_template_metadata_{fn_name}"),
        proc_macro2::Span::call_site(),
    );

    // Generate handler function name
    let handler_fn_name = syn::Ident::new(
        &format!("__turbomcp_template_handler_{fn_name}"),
        proc_macro2::Span::call_site(),
    );

    // Generate public metadata function for testing
    let public_metadata_fn_name = syn::Ident::new(
        &format!("{}_metadata", fn_name),
        proc_macro2::Span::call_site(),
    );

    // Analyze function signature for parameter handling
    let analysis = match analyze_template_signature(fn_sig) {
        Ok(analysis) => analysis,
        Err(err) => return err.to_compile_error().into(),
    };

    let param_extraction = generate_template_parameter_extraction(&analysis);
    let call_args = &analysis.call_args;

    // Implementation that preserves function and enables auto-discovery
    let expanded = quote! {
        // Keep original function unchanged
        #fn_vis #fn_sig #fn_block

        // Generate metadata function
        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #metadata_fn_name() -> (&'static str, &'static str, &'static str, &'static str) {
            (#handler_name, #uri_template, "template", "Resource template handler")
        }

        // Generate public metadata function for testing
        /// Get metadata for this template handler (name, uri_template, type, description)
        pub fn #public_metadata_fn_name() -> (&'static str, &'static str, &'static str, &'static str) {
            Self::#metadata_fn_name()
        }

        // Generate handler function that bridges resource template requests to the actual method
        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #handler_fn_name(&self, uri: String, parameters: std::collections::HashMap<String, serde_json::Value>, context: turbomcp::RequestContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, turbomcp::ServerError>> + Send + '_>> {
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
                    factory.create_for_tool(context.clone(), #handler_name, Some(#uri_template))
                        .await
                        .unwrap_or_else(|_| {
                            // Fallback to basic context if factory fails
                            let handler_metadata = turbomcp::HandlerMetadata {
                                name: #handler_name.to_string(),
                                handler_type: "template".to_string(),
                                description: Some(#uri_template.to_string()),
                            };
                            turbomcp::Context::new(context, handler_metadata)
                        })
                };

                // Extract parameters from URI template parameters
                #param_extraction

                // Call the actual method and convert result
                let result = self.#fn_name(#call_args).await
                    .map_err(|e| match e {
                        turbomcp::McpError::Server(server_err) => server_err,
                        turbomcp::McpError::Tool(msg) => turbomcp::ServerError::handler(msg),
                        turbomcp::McpError::Resource(msg) => turbomcp::ServerError::handler(msg),
                        turbomcp::McpError::Prompt(msg) => turbomcp::ServerError::handler(msg),
                        turbomcp::McpError::Protocol(msg) => turbomcp::ServerError::handler(msg),
                        turbomcp::McpError::Context(msg) => turbomcp::ServerError::handler(msg),
                        turbomcp::McpError::Unauthorized(msg) => turbomcp::ServerError::authorization(msg),
                        turbomcp::McpError::Network(msg) => turbomcp::ServerError::handler(msg),
                        turbomcp::McpError::InvalidInput(msg) => turbomcp::ServerError::handler(msg),
                        turbomcp::McpError::Schema(msg) => turbomcp::ServerError::handler(msg),
                        turbomcp::McpError::Transport(msg) => turbomcp::ServerError::handler(msg),
                        turbomcp::McpError::Serialization(e) => turbomcp::ServerError::from(e),
                        turbomcp::McpError::Internal(msg) => turbomcp::ServerError::Internal(msg),
                        turbomcp::McpError::InvalidRequest(msg) => turbomcp::ServerError::handler(msg),
                    })?;

                // Convert result to string content - properly serialize the result
                let content = match ::serde_json::to_value(&result) {
                    Ok(val) if val.is_string() => {
                        // If result is already a string, use it directly
                        val.as_str().unwrap_or("").to_string()
                    }
                    Ok(val) => {
                        // For other types, use JSON representation
                        ::serde_json::to_string(&val).unwrap_or_else(|_| format!("{:?}", result))
                    }
                    Err(_) => {
                        // Fallback to Debug (Display not guaranteed for all types)
                        format!("{:?}", result)
                    }
                };

                Ok(content)
            })
        }
    };

    expanded.into()
}

/// Analysis result for template function signature
#[derive(Debug)]
struct TemplateAnalysis {
    /// Arguments to pass to the function call
    call_args: TokenStream2,
    /// Parameters that need extraction from URI template
    parameters: Vec<(String, Type)>,
}

/// Analyze template function signature
fn analyze_template_signature(sig: &Signature) -> syn::Result<TemplateAnalysis> {
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
                        // This is a regular parameter that needs extraction from URI template
                        call_args.push(quote! { #ident });
                        parameters.push((param_name, ty.as_ref().clone()));
                    }
                } else {
                    return Err(syn::Error::new_spanned(
                        pat,
                        "Complex patterns not supported in template handlers",
                    ));
                }
            }
        }
    }

    let call_args = quote! { #(#call_args),* };

    Ok(TemplateAnalysis {
        call_args,
        parameters,
    })
}

/// Generate parameter extraction code for template
fn generate_template_parameter_extraction(analysis: &TemplateAnalysis) -> TokenStream2 {
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
                    let #ident: #ty = parameters.get(#name)
                        .and_then(|v| serde_json::from_value(v.clone()).ok());
                }
            } else {
                // For required parameters, fail if not present
                quote! {
                    let #ident: #ty = parameters.get(#name)
                        .and_then(|v| serde_json::from_value(v.clone()).ok())
                        .ok_or_else(|| turbomcp::ServerError::handler(
                            format!("Missing required template parameter: {}", #name)
                        ))?;
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
