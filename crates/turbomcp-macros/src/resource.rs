//! Production-grade resource macro implementation with comprehensive argument parsing

use crate::uri_template::UriTemplate;
use proc_macro::TokenStream;
use quote::quote;
use syn::{
    ItemFn, Lit, Meta, Token, parse::Parse, parse::ParseStream, parse_macro_input,
    punctuated::Punctuated,
};

/// Comprehensive resource configuration for maximum utility and DX
#[derive(Debug, Default)]
struct ResourceConfig {
    /// Human-readable display name (e.g., "Document Content")
    name: Option<String>,
    /// Optional title for display purposes (human-readable)
    title: Option<String>,
    /// Optional description of what this resource provides
    description: Option<String>,
    /// URI template with parameters (e.g., "docs://content/{name}")
    uri_template: Option<String>,
    /// Content MIME type
    mime_type: Option<String>,
    /// Resource tags for categorization
    tags: Vec<String>,
}

/// Production-grade attribute parser for comprehensive resource configuration
struct ResourceArgs {
    items: Punctuated<Meta, Token![,]>,
}

impl Parse for ResourceArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(ResourceArgs {
            items: input.parse_terminated(Meta::parse, Token![,])?,
        })
    }
}

/// Generate production-grade resource implementation with comprehensive argument processing
pub fn generate_resource_impl(args: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as ItemFn);

    // Production-grade argument parsing with comprehensive validation
    let config = match parse_resource_args(args, &input.sig.ident) {
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

    // Get URI template and validate it
    let uri_template = config
        .uri_template
        .unwrap_or_else(|| format!("resource://{}", fn_name));

    // Parse URI template for intelligent name generation
    let parsed_template = match UriTemplate::parse(&uri_template) {
        Ok(template) => template,
        Err(e) => {
            return syn::Error::new_spanned(
                &input.sig.ident,
                format!("Invalid URI template '{}': {}", uri_template, e),
            )
            .to_compile_error()
            .into();
        }
    };

    // Generate intelligent display name if not provided
    let display_name = config
        .name
        .unwrap_or_else(|| parsed_template.generate_name());

    // Generate human-readable title for display purposes
    let title = config.title.unwrap_or_else(|| {
        if parsed_template.is_parameterized() {
            format!("{} Resource", display_name)
        } else {
            display_name.clone()
        }
    });

    // Use provided description or generate from URI template
    let description = config.description.unwrap_or_else(|| {
        if parsed_template.is_parameterized() {
            format!(
                "Access {} with parameters: {}",
                display_name.to_lowercase(),
                parsed_template.variables().join(", ")
            )
        } else {
            format!("Access {}", display_name.to_lowercase())
        }
    });

    // Get MIME type
    let mime_type = config.mime_type.unwrap_or_else(|| "text/plain".to_string());

    // Generate comprehensive metadata function
    let metadata_fn_name = syn::Ident::new(
        &format!("__turbomcp_resource_metadata_{fn_name}"),
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
        &format!("__turbomcp_resource_handler_{fn_name}"),
        proc_macro2::Span::call_site(),
    );

    // Analyze function signature for resource parameter extraction
    let analysis = match analyze_resource_signature(fn_sig) {
        Ok(analysis) => analysis,
        Err(err) => return err.to_compile_error().into(),
    };

    let param_extraction = generate_resource_parameter_extraction(&analysis, &metadata_fn_name);
    let call_args = &analysis.call_args;

    // Production-grade implementation with comprehensive metadata support
    let expanded = quote! {
        // Preserve original function with all its attributes
        #fn_vis #fn_sig #fn_block

        // Generate comprehensive metadata function for internal use
        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #metadata_fn_name() -> (&'static str, &'static str, &'static str, &'static str, &'static str, Vec<String>) {
            (
                #uri_template,      // URI template for matching
                #display_name,      // Human-readable name
                #title,             // Display title (MCP spec)
                #description,       // Description
                #mime_type,         // MIME type
                #tags_tokens        // Tags
            )
        }

        // Generate public metadata function for testing and integration
        /// Get comprehensive metadata for this resource
        ///
        /// Returns (uri_template, name, title, description, mime_type, tags) tuple providing complete
        /// resource metadata for testing, documentation, and runtime introspection.
        pub fn #public_metadata_fn_name() -> (&'static str, &'static str, &'static str, &'static str, &'static str, Vec<String>) {
            Self::#metadata_fn_name()
        }

        // Generate handler function that bridges ReadResourceRequest to the actual method
        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #handler_fn_name(&self, request: ::turbomcp::turbomcp_protocol::ReadResourceRequest, context: ::turbomcp::RequestContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, ::turbomcp::ServerError>> + Send + '_>> {
            Box::pin(async move {
                // Context injection using ContextFactory pattern
                let turbomcp_ctx = {
                    use ::turbomcp::{ContextFactory, ContextFactoryConfig, Container};

                    let config = ContextFactoryConfig {
                        enable_tracing: true,
                        enable_metrics: true,
                        max_pool_size: 50,
                        default_strategy: ::turbomcp::ContextCreationStrategy::Inherit,
                        ..Default::default()
                    };
                    let container = Container::new();
                    let factory = ContextFactory::new(config, container);

                    factory.create_for_resource(context.clone(), #display_name)
                        .await
                        .unwrap_or_else(|_| {
                            let handler_metadata = ::turbomcp::HandlerMetadata {
                                name: #display_name.to_string(),
                                handler_type: "resource".to_string(),
                                description: Some(#description.to_string()),
                            };
                            ::turbomcp::Context::new(context, handler_metadata)
                        })
                };

                #param_extraction

                // Call the actual method with extracted parameters
                let result = self.#fn_name(#call_args).await
                    .map_err(|e| match e {
                        ::turbomcp::McpError::Server(server_err) => server_err,
                        ::turbomcp::McpError::Resource(msg) => ::turbomcp::ServerError::handler(msg),
                        ::turbomcp::McpError::Tool(msg) => ::turbomcp::ServerError::handler(msg),
                        ::turbomcp::McpError::Prompt(msg) => ::turbomcp::ServerError::handler(msg),
                        ::turbomcp::McpError::Protocol(msg) => ::turbomcp::ServerError::handler(msg),
                        ::turbomcp::McpError::Context(msg) => ::turbomcp::ServerError::handler(msg),
                        ::turbomcp::McpError::Unauthorized(msg) => ::turbomcp::ServerError::authorization(msg),
                        ::turbomcp::McpError::Network(msg) => ::turbomcp::ServerError::handler(msg),
                        ::turbomcp::McpError::InvalidInput(msg) => ::turbomcp::ServerError::handler(msg),
                        ::turbomcp::McpError::Schema(msg) => ::turbomcp::ServerError::handler(msg),
                        ::turbomcp::McpError::Transport(msg) => ::turbomcp::ServerError::handler(msg),
                        ::turbomcp::McpError::Serialization(e) => ::turbomcp::ServerError::from(e),
                        ::turbomcp::McpError::Internal(msg) => ::turbomcp::ServerError::Internal(msg),
                        ::turbomcp::McpError::InvalidRequest(msg) => ::turbomcp::ServerError::handler(msg),
                    })?;

                Ok(result)
            })
        }
    };

    TokenStream::from(expanded)
}

/// Production-grade argument parsing with progressive enhancement: simple to advanced usage
fn parse_resource_args(
    args: TokenStream,
    _fn_ident: &syn::Ident,
) -> Result<ResourceConfig, String> {
    if args.is_empty() {
        // #[resource] - simplest usage, function name becomes resource name
        return Ok(ResourceConfig {
            name: None,
            title: None,
            description: None,
            uri_template: None,
            mime_type: None,
            tags: vec![],
        });
    }

    let args: proc_macro2::TokenStream = args.into();

    // First, try parsing as a simple string literal: #[resource("uri_template")]
    if let Ok(lit_str) = syn::parse2::<syn::LitStr>(args.clone()) {
        return Ok(ResourceConfig {
            name: None,
            title: None,
            description: None,
            uri_template: Some(lit_str.value()),
            mime_type: None,
            tags: vec![],
        });
    }

    // Next, try parsing as structured arguments: #[resource(uri = "...", name = "...", tags = [...])]
    let parsed_args = match syn::parse2::<ResourceArgs>(args) {
        Ok(args) => args,
        Err(e) => {
            return Err(format!(
                "Invalid resource macro arguments. Use:\n  #[resource] for default\n  #[resource(\"uri_template\")] for simple URI\n  #[resource(uri = \"...\", name = \"...\", tags = [...])] for advanced\nError: {}",
                e
            ));
        }
    };

    let mut config = ResourceConfig::default();

    // Process each attribute with comprehensive validation
    for meta in &parsed_args.items {
        match meta {
            // Handle named attributes: #[resource(name = "...", uri = "...", tags = [...])]
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
                                return Err("Resource name must be a string literal".to_string());
                            }
                        } else {
                            return Err("Resource name must be a string literal".to_string());
                        }
                    }
                    "title" => {
                        if let syn::Expr::Lit(expr_lit) = &name_value.value {
                            if let Lit::Str(lit_str) = &expr_lit.lit {
                                config.title = Some(lit_str.value());
                            } else {
                                return Err("Resource title must be a string literal".to_string());
                            }
                        } else {
                            return Err("Resource title must be a string literal".to_string());
                        }
                    }
                    "description" => {
                        if let syn::Expr::Lit(expr_lit) = &name_value.value {
                            if let Lit::Str(lit_str) = &expr_lit.lit {
                                config.description = Some(lit_str.value());
                            } else {
                                return Err(
                                    "Resource description must be a string literal".to_string()
                                );
                            }
                        } else {
                            return Err("Resource description must be a string literal".to_string());
                        }
                    }
                    "mime_type" | "mimeType" => {
                        if let syn::Expr::Lit(expr_lit) = &name_value.value {
                            if let Lit::Str(lit_str) = &expr_lit.lit {
                                config.mime_type = Some(lit_str.value());
                            } else {
                                return Err(
                                    "Resource MIME type must be a string literal".to_string()
                                );
                            }
                        } else {
                            return Err("Resource MIME type must be a string literal".to_string());
                        }
                    }
                    "uri" | "uri_template" => {
                        if let syn::Expr::Lit(expr_lit) = &name_value.value {
                            if let Lit::Str(lit_str) = &expr_lit.lit {
                                config.uri_template = Some(lit_str.value());
                            } else {
                                return Err(
                                    "Resource URI template must be a string literal".to_string()
                                );
                            }
                        } else {
                            return Err(
                                "Resource URI template must be a string literal".to_string()
                            );
                        }
                    }
                    _ => {
                        return Err(format!(
                            "Unknown resource attribute: {}. Supported: name, title, description, mime_type, uri, tags",
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

            // Handle path-only syntax (not supported, guide user to clear syntax)
            Meta::Path(_) => {
                return Err("Use #[resource(uri = \"template\")] for structured syntax".to_string());
            }
        }
    }

    Ok(config)
}

/// Analysis of resource function signature
struct ResourceFunctionAnalysis {
    parameters: Vec<ResourceParameterInfo>,
    call_args: proc_macro2::TokenStream,
}

/// Information about a resource parameter
struct ResourceParameterInfo {
    name: String,
    ty: syn::Type,
}

/// Analyze resource function signature to extract parameters and generate appropriate code
fn analyze_resource_signature(
    sig: &syn::Signature,
) -> Result<ResourceFunctionAnalysis, syn::Error> {
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
                        parameters.push(ResourceParameterInfo {
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

    Ok(ResourceFunctionAnalysis {
        parameters,
        call_args,
    })
}

/// Generate parameter extraction code for resources with compile-time URI template parsing
fn generate_resource_parameter_extraction(
    analysis: &ResourceFunctionAnalysis,
    metadata_fn_name: &syn::Ident,
) -> proc_macro2::TokenStream {
    if analysis.parameters.is_empty() {
        return quote! {};
    }

    let mut extraction_code = quote! {};

    // Generate efficient compile-time URI parsing based on the specific template
    extraction_code.extend(quote! {
        let uri = &request.uri;

        // Get the URI template at compile time for parameter extraction
        let (uri_template, _, _, _, _, _) = Self::#metadata_fn_name();

        // Simple but effective URI parameter extraction
        let extracted_params = extract_uri_parameters(uri, uri_template);
    });

    for param in &analysis.parameters {
        let param_name_str = &param.name;
        let param_name_ident = syn::Ident::new(&param.name, proc_macro2::Span::call_site());
        let param_ty = &param.ty;

        if param_name_str == "uri" {
            // Special case: if parameter is named 'uri', pass the full URI
            extraction_code.extend(quote! {
                let #param_name_ident: #param_ty = uri.clone();
            });
        } else {
            // Extract parameter from URI template matching
            extraction_code.extend(quote! {
                let #param_name_ident: #param_ty = extracted_params
                    .get(#param_name_str)
                    .cloned()
                    .unwrap_or_default();
            });
        }
    }

    // Add helper function for parameter extraction
    extraction_code.extend(quote! {
        fn extract_uri_parameters(uri: &str, template: &str) -> std::collections::HashMap<String, String> {
            let mut params = std::collections::HashMap::new();

            if !template.contains('{') {
                return params; // No variables in template
            }

            // Parse template and URI parts
            let template_parts: Vec<&str> = template.split('/').filter(|s| !s.is_empty()).collect();
            let uri_parts: Vec<&str> = uri.split('/').filter(|s| !s.is_empty()).collect();

            if template_parts.len() != uri_parts.len() {
                return params; // Length mismatch
            }

            for (template_part, uri_part) in template_parts.iter().zip(uri_parts.iter()) {
                if template_part.starts_with('{') && template_part.ends_with('}') {
                    // Extract variable name and value
                    let var_name = &template_part[1..template_part.len()-1];
                    params.insert(var_name.to_string(), uri_part.to_string());
                }
            }

            params
        }
    });

    extraction_code
}
