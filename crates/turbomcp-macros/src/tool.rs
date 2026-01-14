//! Tool macro - generates JSON schema and handler code from function signature

use crate::attrs::ToolAttrs;
use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{FnArg, ItemFn, Pat, PatType, Signature, Type, parse_macro_input};

/// Generate tool implementation with auto-discovery
pub fn generate_tool_impl(args: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as ItemFn);

    // Parse attributes using structured parser
    let tool_attrs = match ToolAttrs::from_args(args) {
        Ok(attrs) => attrs,
        Err(err) => return err.to_compile_error().into(),
    };

    // Combine all fields into single description string (MCP spec-compliant)
    let description = tool_attrs.combine_description();

    let fn_name = &input.sig.ident;
    let fn_vis = &input.vis;
    let fn_block = &input.block;
    let fn_sig = &input.sig;
    let tool_name = fn_name.to_string();

    // Generate metadata function that can be tested
    let metadata_fn_name = syn::Ident::new(
        &format!("__turbomcp_tool_metadata_{fn_name}"),
        proc_macro2::Span::call_site(),
    );

    // Analyze function signature for schema generation
    let analysis = match analyze_function_signature(fn_sig) {
        Ok(analysis) => analysis,
        Err(err) => return err.to_compile_error().into(),
    };

    let schema_generation = generate_schema(&analysis);

    // Generate parameter extraction code
    let param_extraction = generate_parameter_extraction(&analysis);
    let call_args = &analysis.call_args;

    // Generate handler function name
    let handler_fn_name = syn::Ident::new(
        &format!("__turbomcp_tool_handler_{fn_name}"),
        proc_macro2::Span::call_site(),
    );

    // Generate public metadata function for testing
    let public_metadata_fn_name = syn::Ident::new(
        &format!("{}_metadata", fn_name),
        proc_macro2::Span::call_site(),
    );

    let expanded = quote! {
        #fn_vis #fn_sig #fn_block

        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #metadata_fn_name() -> (&'static str, &'static str, turbomcp::ToolInputSchema) {
            let schema_json = #schema_generation;
            let schema: turbomcp::ToolInputSchema = serde_json::from_value(schema_json)
                .expect("Generated schema should always be valid ToolInputSchema");
            (#tool_name, #description, schema)
        }

        /// Returns (name, description, schema) for testing
        pub fn #public_metadata_fn_name() -> (&'static str, &'static str, turbomcp::ToolInputSchema) {
            Self::#metadata_fn_name()
        }

        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #handler_fn_name(&self, request: turbomcp::CallToolRequest, context: turbomcp::RequestContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<turbomcp::CallToolResult, ::turbomcp::__macro_support::turbomcp_server::McpError>> + Send + '_>> {
            Box::pin(async move {
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

                    factory.create_for_tool(context.clone(), #tool_name, Some(#description))
                        .await
                        .unwrap_or_else(|_| {
                            let handler_metadata = turbomcp::HandlerMetadata {
                                name: #tool_name.to_string(),
                                handler_type: "tool".to_string(),
                                description: Some(#description.to_string()),
                            };
                            turbomcp::Context::new(context, handler_metadata)
                        })
                };

                #param_extraction

                // Execute the handler and serialize result to JSON
                // This maintains backwards compatibility with existing handlers that return
                // any Serialize type (e.g., custom structs, Vec<T>, etc.)
                //
                // For ergonomic returns (String, i32, Json<T>, etc.), users can import
                // IntoToolResponse from turbomcp::prelude and use those types directly.
                let result = self.#fn_name(#call_args).await?;

                let text = match ::serde_json::to_value(&result) {
                    Ok(val) if val.is_string() => val.as_str().unwrap_or("").to_string(),
                    Ok(val) => ::serde_json::to_string(&val).unwrap_or_else(|_| format!("{:?}", result)),
                    Err(_) => format!("{:?}", result),
                };

                Ok(turbomcp::CallToolResult {
                    content: vec![turbomcp::Content::Text(turbomcp::TextContent {
                        text,
                        annotations: None,
                        meta: None,
                    })],
                    is_error: Some(false),
                    structured_content: None,
                    _meta: None,
                    task_id: None,
                })
            })
        }
    };

    TokenStream::from(expanded)
}

struct FunctionAnalysis {
    parameters: Vec<ParameterInfo>,
    #[allow(dead_code)]
    call_args: TokenStream2,
    #[allow(dead_code)]
    has_self: bool,
}

#[derive(Clone)]
struct ParameterInfo {
    name: String,
    ty: Type,
    #[allow(dead_code)]
    doc: Option<String>,
}

fn analyze_function_signature(sig: &Signature) -> Result<FunctionAnalysis, syn::Error> {
    let mut parameters = Vec::new();
    let mut call_args = TokenStream2::new();
    let mut has_self = false;
    let mut first_param = true;

    for input in &sig.inputs {
        match input {
            FnArg::Receiver(_) => {
                has_self = true;
                continue;
            }
            FnArg::Typed(PatType { pat, ty, .. }) => {
                if let Pat::Ident(pat_ident) = pat.as_ref() {
                    let param_name = &pat_ident.ident;

                    let is_context = if let Type::Path(type_path) = ty.as_ref() {
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
                        parameters.push(ParameterInfo {
                            name: param_name.to_string(),
                            ty: (**ty).clone(),
                            doc: None,
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

    Ok(FunctionAnalysis {
        parameters,
        call_args,
        has_self,
    })
}

#[allow(dead_code)]
fn generate_parameter_extraction(analysis: &FunctionAnalysis) -> TokenStream2 {
    if analysis.parameters.is_empty() {
        return quote! {};
    }

    let mut extraction_code = quote! {};

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

        let is_optional = is_option_type(&param.ty);

        if is_optional {
            extraction_code.extend(quote! {
                let #param_name_ident: #param_ty = if let Some(args) = arguments {
                    args.get(#param_name_str)
                        .map(|v| ::serde_json::from_value(v.clone())
                            .map_err(|e| ::turbomcp::__macro_support::turbomcp_server::McpError::internal(
                                format!("Invalid parameter {}: {}", #param_name_str, e)
                            )))
                        .transpose()?
                        .flatten()
                } else {
                    None
                };
            });
        } else {
            extraction_code.extend(quote! {
                let #param_name_ident = arguments
                    .as_ref()
                    .ok_or_else(|| ::turbomcp::__macro_support::turbomcp_server::McpError::internal("Missing arguments"))?
                    .get(#param_name_str)
                    .ok_or_else(|| ::turbomcp::__macro_support::turbomcp_server::McpError::internal(
                        format!("Missing required parameter: {}", #param_name_str)
                    ))?;
                let #param_name_ident: #param_ty = ::serde_json::from_value(#param_name_ident.clone())
                    .map_err(|e| ::turbomcp::__macro_support::turbomcp_server::McpError::internal(
                        format!("Invalid parameter {}: {}", #param_name_str, e)
                    ))?;
            });
        }
    }

    extraction_code
}

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

fn generate_schema(analysis: &FunctionAnalysis) -> TokenStream2 {
    if analysis.parameters.is_empty() {
        return quote! {
            {
                let mut schema_map = ::serde_json::Map::new();
                schema_map.insert("type".to_string(), ::serde_json::Value::String("object".to_string()));
                schema_map.insert("properties".to_string(), ::serde_json::Value::Object(::serde_json::Map::new()));
                schema_map.insert("required".to_string(), ::serde_json::Value::Array(Vec::new()));
                schema_map.insert("additionalProperties".to_string(), ::serde_json::Value::Bool(false));
                ::serde_json::Value::Object(schema_map)
            }
        };
    }

    let mut prop_entries: Vec<(syn::LitStr, TokenStream2)> = Vec::new();
    let mut required_entries: Vec<syn::LitStr> = Vec::new();

    for p in &analysis.parameters {
        let key = syn::LitStr::new(&p.name, proc_macro2::Span::call_site());
        let schema_ts =
            crate::schema::generate_json_schema_with_description(&p.ty, p.doc.as_deref());
        prop_entries.push((key.clone(), schema_ts));

        let is_optional = is_option_type(&p.ty);
        if !is_optional {
            required_entries.push(key);
        }
    }

    let keys: Vec<syn::LitStr> = prop_entries.iter().map(|(k, _)| k.clone()).collect();
    let values: Vec<TokenStream2> = prop_entries.iter().map(|(_, v)| v.clone()).collect();

    quote! {
        {
            let mut schema_map = ::serde_json::Map::new();
            schema_map.insert("type".to_string(), ::serde_json::Value::String("object".to_string()));

            let mut properties_map = ::serde_json::Map::new();
            #(
                properties_map.insert(#keys.to_string(), #values);
            )*
            schema_map.insert("properties".to_string(), ::serde_json::Value::Object(properties_map));

            let required_array = vec![#(::serde_json::Value::String(#required_entries.to_string())),*];
            schema_map.insert("required".to_string(), ::serde_json::Value::Array(required_array));
            schema_map.insert("additionalProperties".to_string(), ::serde_json::Value::Bool(false));

            ::serde_json::Value::Object(schema_map)
        }
    }
}
