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
        fn #handler_fn_name(&self, uri: String, parameters: std::collections::HashMap<String, serde_json::Value>, context: turbomcp_core::RequestContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, turbomcp_server::ServerError>> + Send + '_>> {
            Box::pin(async move {
                // Extract parameters from URI template parameters
                #param_extraction

                // Call the actual method
                let result = self.#fn_name(#call_args).await;

                // Convert result to string content
                match result {
                    Ok(content) => Ok(content),
                    Err(e) => Err(turbomcp_server::ServerError::Handler {
                        message: format!("Template handler failed: {}", e),
                        context: Some(context),
                    }),
                }
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
                        call_args.push(quote! { context });
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
    let extractions: Vec<TokenStream2> = analysis
        .parameters
        .iter()
        .map(|(name, ty)| {
            let ident = syn::Ident::new(name, proc_macro2::Span::call_site());
            quote! {
                let #ident: #ty = parameters.get(#name)
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .ok_or_else(|| turbomcp_server::ServerError::Handler {
                        message: format!("Missing required template parameter: {}", #name),
                        context: Some(context.clone()),
                    })?;
            }
        })
        .collect();

    quote! {
        #(#extractions)*
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
