//! Production-grade prompt macro implementation with comprehensive argument parsing

use proc_macro::TokenStream;
use quote::quote;
use syn::{
    ItemFn, Lit, Meta, Token, parse::Parse, parse::ParseStream, parse_macro_input,
    punctuated::Punctuated,
};

/// Comprehensive prompt configuration for maximum utility and DX
#[derive(Debug, Default)]
struct PromptConfig {
    name: Option<String>,
    description: String,
    tags: Vec<String>,
}

/// Production-grade attribute parser for comprehensive prompt configuration
struct PromptArgs {
    items: Punctuated<Meta, Token![,]>,
}

impl Parse for PromptArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(PromptArgs {
            items: input.parse_terminated(Meta::parse, Token![,])?,
        })
    }
}

/// Generate production-grade prompt implementation with comprehensive argument processing
pub fn generate_prompt_impl(args: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as ItemFn);

    // Production-grade argument parsing with comprehensive validation
    let config = match parse_prompt_args(args) {
        Ok(config) => config,
        Err(error) => {
            return syn::Error::new_spanned(&input.sig.ident, error)
                .to_compile_error()
                .into();
        }
    };

    let fn_name = &input.sig.ident;
    let fn_vis = &input.vis;
    let fn_block = &input.block;
    let fn_sig = &input.sig;
    let prompt_name = config.name.unwrap_or_else(|| fn_name.to_string());
    let description = &config.description;

    // Generate comprehensive metadata function
    let metadata_fn_name = syn::Ident::new(
        &format!("__turbomcp_prompt_metadata_{fn_name}"),
        proc_macro2::Span::call_site(),
    );

    // Generate public metadata function name for testing capability
    let public_metadata_fn_name = syn::Ident::new(
        &format!("{fn_name}_metadata"),
        proc_macro2::Span::call_site(),
    );

    // Generate tags as a vector literal
    let tags_tokens = if config.tags.is_empty() {
        quote! { vec![] }
    } else {
        let tag_strings = &config.tags;
        quote! { vec![#(#tag_strings.to_string()),*] }
    };

    // Generate handler function name
    let handler_fn_name = syn::Ident::new(
        &format!("__turbomcp_prompt_handler_{fn_name}"),
        proc_macro2::Span::call_site(),
    );

    // Analyze function signature for prompt parameter extraction
    let analysis = match analyze_prompt_signature(fn_sig) {
        Ok(analysis) => analysis,
        Err(err) => return err.to_compile_error().into(),
    };

    let param_extraction = generate_prompt_parameter_extraction(&analysis);
    let call_args = &analysis.call_args;

    // Generate JSON schema for prompt arguments
    let arguments_schema = generate_prompt_arguments_schema(&analysis);

    // Production-grade implementation with comprehensive metadata support
    let expanded = quote! {
        // Preserve original function with all its attributes
        #fn_vis #fn_sig #fn_block

        // Generate comprehensive metadata function for internal use
        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #metadata_fn_name() -> (&'static str, &'static str, Vec<serde_json::Value>, Vec<String>) {
            (
                #prompt_name,
                #description,
                #arguments_schema,
                #tags_tokens
            )
        }

        // Generate public metadata function for testing and integration
        /// Get comprehensive metadata for this prompt
        ///
        /// Returns (name, description, arguments_schema, tags) tuple providing complete prompt metadata
        /// for testing, documentation, and runtime introspection with maximum utility.
        pub fn #public_metadata_fn_name() -> (&'static str, &'static str, Vec<serde_json::Value>, Vec<String>) {
            Self::#metadata_fn_name()
        }

        // Generate handler function that bridges GetPromptRequest to the actual method
        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #handler_fn_name(&self, request: turbomcp_protocol::GetPromptRequest, context: turbomcp::RequestContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, turbomcp::ServerError>> + Send + '_>> {
            Box::pin(async move {
                // Context injection using ContextFactory pattern
                let turbomcp_ctx = {
                    use turbomcp::{ContextFactory, ContextFactoryConfig, Container};

                    let config = ContextFactoryConfig {
                        enable_tracing: true,
                        enable_metrics: true,
                        max_pool_size: 50,
                        default_strategy: turbomcp::ContextCreationStrategy::Inherit,
                        ..Default::default()
                    };
                    let container = Container::new();
                    let factory = ContextFactory::new(config, container);

                    factory.create_for_prompt(context.clone(), #prompt_name)
                        .await
                        .unwrap_or_else(|_| {
                            let handler_metadata = turbomcp::HandlerMetadata {
                                name: #prompt_name.to_string(),
                                handler_type: "prompt".to_string(),
                                description: Some(#description.to_string()),
                            };
                            turbomcp::Context::new(context, handler_metadata)
                        })
                };

                #param_extraction

                // Call the actual method with extracted parameters
                let result = self.#fn_name(#call_args).await
                    .map_err(|e| match e {
                        turbomcp::McpError::Server(server_err) => server_err,
                        turbomcp::McpError::Prompt(msg) => turbomcp::ServerError::handler(msg),
                        turbomcp::McpError::Tool(msg) => turbomcp::ServerError::handler(msg),
                        turbomcp::McpError::Resource(msg) => turbomcp::ServerError::handler(msg),
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

                Ok(result)
            })
        }
    };

    TokenStream::from(expanded)
}

/// Production-grade argument parsing with progressive enhancement: simple to advanced usage
fn parse_prompt_args(args: TokenStream) -> Result<PromptConfig, String> {
    if args.is_empty() {
        return Err("Prompt description is required for proper documentation".to_string());
    }

    let args: proc_macro2::TokenStream = args.into();

    // First, try parsing as a simple string literal: #[prompt("description")]
    if let Ok(lit_str) = syn::parse2::<syn::LitStr>(args.clone()) {
        return Ok(PromptConfig {
            description: lit_str.value(),
            name: None,
            tags: vec![],
        });
    }

    // Next, try parsing as structured arguments: #[prompt(desc = "...", name = "...", tags = [...])]
    let parsed_args = match syn::parse2::<PromptArgs>(args) {
        Ok(args) => args,
        Err(e) => {
            return Err(format!(
                "Invalid prompt macro arguments. Use:\n  #[prompt(\"description\")] for simple usage\n  #[prompt(desc = \"...\", name = \"...\", tags = [...])] for advanced\nError: {}",
                e
            ));
        }
    };

    let mut config = PromptConfig::default();

    // Process each attribute with comprehensive validation
    for meta in &parsed_args.items {
        match meta {
            // Handle path-only syntax (not supported, guide user to clear syntax)
            Meta::Path(_) => {
                return Err(
                    "Use #[prompt(desc = \"description\")] for structured syntax".to_string(),
                );
            }

            // Handle named attributes: #[prompt(name = "...", desc = "...", tags = [...])]
            Meta::NameValue(name_value) => {
                let attr_name = name_value
                    .path
                    .get_ident()
                    .ok_or_else(|| "Invalid attribute name".to_string())?
                    .to_string();

                match attr_name.as_str() {
                    "name" => {
                        if let syn::Expr::Lit(expr_lit) = &name_value.value {
                            if let Lit::Str(lit_str) = &expr_lit.lit {
                                config.name = Some(lit_str.value());
                            } else {
                                return Err("Prompt name must be a string literal".to_string());
                            }
                        } else {
                            return Err("Prompt name must be a string literal".to_string());
                        }
                    }
                    "desc" | "description" => {
                        if let syn::Expr::Lit(expr_lit) = &name_value.value {
                            if let Lit::Str(lit_str) = &expr_lit.lit {
                                config.description = lit_str.value();
                            } else {
                                return Err(
                                    "Prompt description must be a string literal".to_string()
                                );
                            }
                        } else {
                            return Err("Prompt description must be a string literal".to_string());
                        }
                    }
                    _ => {
                        return Err(format!(
                            "Unknown prompt attribute: {}. Supported: name, desc, tags",
                            attr_name
                        ));
                    }
                }
            }

            // Handle list attributes like tags = ["tag1", "tag2"]
            Meta::List(meta_list) => {
                let attr_name = meta_list
                    .path
                    .get_ident()
                    .ok_or_else(|| "Invalid attribute name".to_string())?
                    .to_string();

                match attr_name.as_str() {
                    "tags" => {
                        // Parse the token stream inside the brackets
                        let tags_content = meta_list.tokens.clone();
                        let bracketed: syn::ExprArray = syn::parse2(quote! { [#tags_content] })
                            .map_err(|_| {
                                "Tags must be an array of strings like [\"tag1\", \"tag2\"]"
                                    .to_string()
                            })?;

                        for expr in bracketed.elems {
                            if let syn::Expr::Lit(expr_lit) = expr {
                                if let Lit::Str(lit_str) = expr_lit.lit {
                                    config.tags.push(lit_str.value());
                                } else {
                                    return Err("Tag values must be string literals".to_string());
                                }
                            } else {
                                return Err("Tag values must be string literals".to_string());
                            }
                        }
                    }
                    _ => {
                        return Err(format!(
                            "Unknown list attribute: {}. Supported: tags",
                            attr_name
                        ));
                    }
                }
            }
        }
    }

    // Final validation
    if config.description.is_empty() {
        return Err("Prompt description is required. Use #[prompt(desc = \"your description\")] or #[prompt(\"description\")]".to_string());
    }

    Ok(config)
}

/// Analysis of prompt function signature
struct PromptFunctionAnalysis {
    parameters: Vec<PromptParameterInfo>,
    call_args: proc_macro2::TokenStream,
}

/// Information about a prompt parameter
struct PromptParameterInfo {
    name: String,
    ty: syn::Type,
}

/// Analyze prompt function signature to extract parameters and generate appropriate code
fn analyze_prompt_signature(sig: &syn::Signature) -> Result<PromptFunctionAnalysis, syn::Error> {
    let mut parameters = Vec::new();
    let mut call_args = proc_macro2::TokenStream::new();
    let mut first_param = true;

    for input in &sig.inputs {
        match input {
            syn::FnArg::Receiver(_) => {
                // &self parameter - skip in call args
                continue;
            }
            syn::FnArg::Typed(syn::PatType { pat, ty, .. }) => {
                if let syn::Pat::Ident(pat_ident) = pat.as_ref() {
                    let param_name = &pat_ident.ident;

                    // Check if this is a Context/RequestContext parameter
                    let is_context = if let syn::Type::Path(type_path) = ty.as_ref() {
                        type_path.path.segments.last().is_some_and(|seg| {
                            seg.ident == "Context" || seg.ident == "RequestContext"
                        })
                    } else {
                        false
                    };

                    if is_context {
                        if !first_param {
                            call_args.extend(quote! { , });
                        }
                        call_args.extend(quote! { turbomcp_ctx });
                    } else {
                        parameters.push(PromptParameterInfo {
                            name: param_name.to_string(),
                            ty: (**ty).clone(),
                        });

                        if !first_param {
                            call_args.extend(quote! { , });
                        }
                        call_args.extend(quote! { #param_name });
                    }

                    first_param = false;
                }
            }
        }
    }

    Ok(PromptFunctionAnalysis {
        parameters,
        call_args,
    })
}

/// Generate parameter extraction code for prompts
fn generate_prompt_parameter_extraction(
    analysis: &PromptFunctionAnalysis,
) -> proc_macro2::TokenStream {
    if analysis.parameters.is_empty() {
        return quote! {};
    }

    let mut extraction_code = quote! {};

    // Check if we have any parameters to extract
    let has_params = !analysis.parameters.is_empty();
    if has_params {
        extraction_code.extend(quote! {
            let arguments = request.arguments.as_ref();
        });
    }

    for param in &analysis.parameters {
        let param_name_str = &param.name;
        let param_name_ident = syn::Ident::new(&param.name, proc_macro2::Span::call_site());
        let param_ty = &param.ty;

        // Check if this is an optional parameter
        let is_optional = is_prompt_option_type(&param.ty);

        if is_optional {
            // For optional parameters, use None if not present
            extraction_code.extend(quote! {
                let #param_name_ident: #param_ty = if let Some(args) = arguments {
                    args.get(#param_name_str)
                        .map(|v| ::serde_json::from_value(v.clone())
                            .map_err(|e| turbomcp::ServerError::handler(
                                format!("Invalid parameter {}: {}", #param_name_str, e)
                            )))
                        .transpose()?
                        .flatten()
                } else {
                    None
                };
            });
        } else {
            // For required parameters, fail if not present
            extraction_code.extend(quote! {
                let #param_name_ident = arguments
                    .as_ref()
                    .ok_or_else(|| turbomcp::ServerError::handler("Missing arguments"))?
                    .get(#param_name_str)
                    .ok_or_else(|| turbomcp::ServerError::handler(
                        format!("Missing required parameter: {}", #param_name_str)
                    ))?;
                let #param_name_ident: #param_ty = ::serde_json::from_value(#param_name_ident.clone())
                    .map_err(|e| turbomcp::ServerError::handler(
                        format!("Invalid parameter {}: {}", #param_name_str, e)
                    ))?;
            });
        }
    }

    extraction_code
}

/// Generate JSON schema for prompt arguments based on function signature analysis
fn generate_prompt_arguments_schema(analysis: &PromptFunctionAnalysis) -> proc_macro2::TokenStream {
    if analysis.parameters.is_empty() {
        return quote! { vec![] };
    }

    let schema_items: Vec<_> = analysis
        .parameters
        .iter()
        .map(|param| {
            let param_name = &param.name;
            let param_type = type_to_json_schema(&param.ty);

            quote! {
                serde_json::json!({
                    "name": #param_name,
                    "description": format!("Parameter: {}", #param_name),
                    "required": true,
                    "schema": #param_type
                })
            }
        })
        .collect();

    quote! {
        vec![#(#schema_items),*]
    }
}

/// Convert Rust type to JSON schema representation
fn type_to_json_schema(ty: &syn::Type) -> proc_macro2::TokenStream {
    if let syn::Type::Path(type_path) = ty
        && let Some(segment) = type_path.path.segments.last()
    {
        match segment.ident.to_string().as_str() {
            "String" | "str" => {
                return quote! {
                    serde_json::json!({
                        "type": "string"
                    })
                };
            }
            "i32" | "i64" | "u32" | "u64" | "isize" | "usize" => {
                return quote! {
                    serde_json::json!({
                        "type": "integer"
                    })
                };
            }
            "f32" | "f64" => {
                return quote! {
                    serde_json::json!({
                        "type": "number"
                    })
                };
            }
            "bool" => {
                return quote! {
                    serde_json::json!({
                        "type": "boolean"
                    })
                };
            }
            _ => {}
        }
    }

    // Default to string for unknown types
    quote! {
        serde_json::json!({
            "type": "string"
        })
    }
}

/// Check if a type is Option<T> for prompts
fn is_prompt_option_type(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Path(type_path) => {
            if let Some(segment) = type_path.path.segments.last() {
                segment.ident == "Option"
            } else {
                false
            }
        }
        _ => false,
    }
}
