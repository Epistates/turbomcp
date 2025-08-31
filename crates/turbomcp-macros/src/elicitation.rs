//! Elicitation macro implementation
//!
//! Provides the #[elicitation] attribute macro for marking methods as elicitation handlers.
//! Elicitation enables servers to request structured input from clients with JSON schema validation.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{FnArg, ItemFn, Pat, PatType, Signature, Type, parse_macro_input};

/// Generate elicitation handler implementation
pub fn generate_elicitation_impl(args: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as ItemFn);

    // Parse description from args
    let raw_args = args.to_string();
    let description = if raw_args.is_empty() {
        format!("Elicitation: {}", input.sig.ident)
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
        &format!("__turbomcp_elicitation_metadata_{fn_name}"),
        proc_macro2::Span::call_site(),
    );

    // Generate handler function name
    let handler_fn_name = syn::Ident::new(
        &format!("__turbomcp_elicitation_handler_{fn_name}"),
        proc_macro2::Span::call_site(),
    );

    // Generate public metadata function for testing
    let public_metadata_fn_name = syn::Ident::new(
        &format!("{}_metadata", fn_name),
        proc_macro2::Span::call_site(),
    );

    // Analyze function signature for parameter handling
    let analysis = match analyze_elicitation_signature(fn_sig) {
        Ok(analysis) => analysis,
        Err(err) => return err.to_compile_error().into(),
    };

    let param_extraction = generate_elicitation_parameter_extraction(&analysis);
    let call_args = &analysis.call_args;

    // Implementation that preserves function and enables auto-discovery
    let expanded = quote! {
        // Keep original function unchanged
        #fn_vis #fn_sig #fn_block

        // Generate metadata function
        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #metadata_fn_name() -> (&'static str, &'static str, &'static str) {
            (#handler_name, #description, "elicitation")
        }

        // Generate public metadata function for testing
        /// Get metadata for this elicitation handler (name, description, type)
        pub fn #public_metadata_fn_name() -> (&'static str, &'static str, &'static str) {
            Self::#metadata_fn_name()
        }

        // Generate handler function that bridges ElicitRequest to the actual method
        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #handler_fn_name(&self, request: turbomcp_protocol::ElicitRequest, context: turbomcp_core::RequestContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<turbomcp_protocol::ElicitResult, turbomcp_server::ServerError>> + Send + '_>> {
            Box::pin(async move {
                // Extract parameters from request
                #param_extraction

                // Call the actual method
                let result = self.#fn_name(#call_args).await;

                // Convert result to ElicitResult
                match result {
                    Ok(value) => Ok(turbomcp_protocol::ElicitResult {
                        content: value,
                        metadata: std::collections::HashMap::new(),
                    }),
                    Err(e) => Err(turbomcp_server::ServerError::Handler {
                        message: format!("Elicitation failed: {}", e),
                        context: Some(context),
                    }),
                }
            })
        }
    };

    expanded.into()
}

/// Analysis result for elicitation function signature
#[derive(Debug)]
struct ElicitationAnalysis {
    /// Arguments to pass to the function call
    call_args: TokenStream2,
    /// Parameters that need extraction from the request
    parameters: Vec<(String, Type)>,
}

/// Analyze elicitation function signature
fn analyze_elicitation_signature(sig: &Signature) -> syn::Result<ElicitationAnalysis> {
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
                        // This is a regular parameter that needs extraction
                        call_args.push(quote! { #ident });
                        parameters.push((param_name, ty.as_ref().clone()));
                    }
                } else {
                    return Err(syn::Error::new_spanned(
                        pat,
                        "Complex patterns not supported in elicitation handlers",
                    ));
                }
            }
        }
    }

    let call_args = quote! { #(#call_args),* };

    Ok(ElicitationAnalysis {
        call_args,
        parameters,
    })
}

/// Generate parameter extraction code for elicitation
fn generate_elicitation_parameter_extraction(analysis: &ElicitationAnalysis) -> TokenStream2 {
    let extractions: Vec<TokenStream2> = analysis.parameters.iter().map(|(name, ty)| {
        let ident = syn::Ident::new(name, proc_macro2::Span::call_site());
        quote! {
            let #ident: #ty = serde_json::from_value(request.arguments.get(#name).cloned().unwrap_or(serde_json::Value::Null))
                .map_err(|e| turbomcp_server::ServerError::Handler {
                    message: format!("Failed to deserialize parameter '{}': {}", #name, e),
                    context: Some(context.clone()),
                })?;
        }
    }).collect();

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
