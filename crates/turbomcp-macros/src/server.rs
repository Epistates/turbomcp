//! Server macro implementation - auto-discovers #[tool], #[resource], #[prompt] methods

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{Ident, ItemImpl, visit::Visit};

/// Visitor that detects stdio-unsafe macros (println!, etc.) in server impl
struct StdioSafetyValidator {
    errors: Vec<StdioViolation>,
}

/// A detected violation of stdio safety
#[derive(Debug, Clone)]
struct StdioViolation {
    macro_name: String,
    line_hint: String,
}

impl StdioSafetyValidator {
    fn new() -> Self {
        Self { errors: Vec::new() }
    }

    fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    fn format_error_message(&self) -> String {
        let violations = self
            .errors
            .iter()
            .map(|v| format!("  {}: {}", v.macro_name, v.line_hint))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            "❌ stdio transport detected but found unsafe stdout writes:\n\n{}\n\n\
            Stdio transport reserves stdout for MCP protocol messages.\n\
            All output MUST go to stderr:\n\n\
            ✅ CORRECT:\n\
               eprintln!(\"debug info\");\n\
               tracing::info!(\"message\");  // if configured for stderr\n\n\
            ❌ WRONG:\n\
               println!(\"debug info\");\n\
               dbg!(value);  // writes to stdout\n\
               std::io::stdout().write_all(b\"...\");\n\n\
            See: https://docs.modelcontextprotocol.io/guides/stdio-output",
            violations
        )
    }
}

impl Visit<'_> for StdioSafetyValidator {
    fn visit_macro(&mut self, node: &syn::Macro) {
        // Check if this is a macro we need to validate
        if let Some(ident) = node.path.get_ident() {
            let macro_name = ident.to_string();
            match macro_name.as_str() {
                "println" => {
                    self.errors.push(StdioViolation {
                        macro_name: "println!()".to_string(),
                        line_hint: "forbidden in stdio server (use eprintln! or tracing)"
                            .to_string(),
                    });
                }
                "print" => {
                    self.errors.push(StdioViolation {
                        macro_name: "print!()".to_string(),
                        line_hint: "forbidden in stdio server (use eprintln! or tracing)"
                            .to_string(),
                    });
                }
                "dbg" => {
                    self.errors.push(StdioViolation {
                        macro_name: "dbg!()".to_string(),
                        line_hint: "forbidden in stdio server (writes to stdout, use tracing)"
                            .to_string(),
                    });
                }
                _ => {}
            }
        }

        // Continue visiting child nodes
        syn::visit::visit_macro(self, node);
    }
}

/// Validate that servers using stdio transport don't use println! or similar stdout macros
fn validate_stdio_safety(
    impl_block: &ItemImpl,
    transports: &Option<Vec<String>>,
) -> Result<(), syn::Error> {
    // Check if stdio is in the transports list
    // If transports is None, it defaults to stdio only (matching default features)
    let should_check = match transports {
        Some(transports) => transports.contains(&"stdio".to_string()),
        None => true, // Default is stdio, so check for safety
    };

    if !should_check {
        return Ok(());
    }

    // Walk the AST looking for unsafe macros
    let mut validator = StdioSafetyValidator::new();
    validator.visit_item_impl(impl_block);

    if validator.has_errors() {
        return Err(syn::Error::new_spanned(
            impl_block,
            validator.format_error_message(),
        ));
    }

    Ok(())
}

/// Generate the TurboMCP server implementation (idiomatic impl block pattern)
pub fn generate_server_impl(args: TokenStream, input_impl: ItemImpl) -> TokenStream {
    // Parse server attributes using syn parsing
    let attrs = match crate::attrs::ServerAttrs::from_args(args) {
        Ok(attrs) => attrs,
        Err(e) => return e.to_compile_error().into(),
    };

    // Validate stdio transport safety (no println! in stdio servers)
    if let Err(e) = validate_stdio_safety(&input_impl, &attrs.transports) {
        return e.to_compile_error().into();
    }

    // Extract the struct name from the impl block
    let struct_name = match &*input_impl.self_ty {
        syn::Type::Path(type_path) => match type_path.path.segments.last() {
            Some(segment) => &segment.ident,
            None => {
                return syn::Error::new_spanned(
                    &type_path.path,
                    "Expected a valid type path with at least one segment",
                )
                .to_compile_error()
                .into();
            }
        },
        _ => {
            return syn::Error::new(
                proc_macro2::Span::call_site(),
                "The #[server] attribute only supports named types",
            )
            .to_compile_error()
            .into();
        }
    };

    // Analyze impl block for #[tool], #[prompt], and #[resource] methods
    let mut tool_methods = Vec::new();
    let mut tool_metadata_functions = Vec::new();
    let mut tool_handler_functions = Vec::new();

    let mut prompt_methods = Vec::new();
    let mut prompt_metadata_functions = Vec::new();
    let mut prompt_handler_functions = Vec::new();

    let mut resource_methods = Vec::new();
    let mut resource_metadata_functions = Vec::new();
    let mut resource_handler_functions = Vec::new();

    for item in &input_impl.items {
        if let syn::ImplItem::Fn(method) = item {
            let method_name = &method.sig.ident;

            // Check for different MCP attributes
            for attr in &method.attrs {
                if attr.path().is_ident("tool") {
                    let metadata_fn_name = Ident::new(
                        &format!("__turbomcp_tool_metadata_{method_name}"),
                        Span::call_site(),
                    );
                    let handler_fn_name = Ident::new(
                        &format!("__turbomcp_tool_handler_{method_name}"),
                        Span::call_site(),
                    );
                    tool_methods.push(method_name.clone());
                    tool_metadata_functions.push(metadata_fn_name);
                    tool_handler_functions.push(handler_fn_name);
                    break;
                } else if attr.path().is_ident("prompt") {
                    let metadata_fn_name = Ident::new(
                        &format!("__turbomcp_prompt_metadata_{method_name}"),
                        Span::call_site(),
                    );
                    let handler_fn_name = Ident::new(
                        &format!("__turbomcp_prompt_handler_{method_name}"),
                        Span::call_site(),
                    );
                    prompt_methods.push(method_name.clone());
                    prompt_metadata_functions.push(metadata_fn_name);
                    prompt_handler_functions.push(handler_fn_name);
                    break;
                } else if attr.path().is_ident("resource") {
                    let metadata_fn_name = Ident::new(
                        &format!("__turbomcp_resource_metadata_{method_name}"),
                        Span::call_site(),
                    );
                    let handler_fn_name = Ident::new(
                        &format!("__turbomcp_resource_handler_{method_name}"),
                        Span::call_site(),
                    );
                    resource_methods.push(method_name.clone());
                    resource_metadata_functions.push(metadata_fn_name);
                    resource_handler_functions.push(handler_fn_name);
                    break;
                }
            }
        }
    }

    // Generate metadata function for testing and runtime
    let metadata_fn_name = Ident::new(
        &format!("__turbomcp_server_metadata_{struct_name}"),
        Span::call_site(),
    );

    let name_value = attrs
        .name
        .clone()
        .unwrap_or_else(|| struct_name.to_string());
    let version_value = attrs.version.clone().unwrap_or_else(|| "1.0.0".to_string());
    let description_value = match &attrs.description {
        Some(desc) => quote! { Some(#desc) },
        None => quote! { None },
    };

    // Generate roots configuration code using the attrs module
    let roots_config = attrs.generate_roots_config();

    // Generate protocol version configuration code
    let protocol_version_config = attrs.generate_protocol_version_config();

    // Prepare tool method data for router generation
    let tool_method_data: Vec<_> = tool_methods
        .iter()
        .zip(tool_metadata_functions.iter())
        .zip(tool_handler_functions.iter())
        .map(|((method, metadata), handler)| (method.clone(), metadata.clone(), handler.clone()))
        .collect();

    // Prepare prompt method data for router generation
    let prompt_method_data: Vec<_> = prompt_methods
        .iter()
        .zip(prompt_metadata_functions.iter())
        .zip(prompt_handler_functions.iter())
        .map(|((method, metadata), handler)| (method.clone(), metadata.clone(), handler.clone()))
        .collect();

    // Prepare resource method data for router generation
    let resource_method_data: Vec<_> = resource_methods
        .iter()
        .zip(resource_metadata_functions.iter())
        .zip(resource_handler_functions.iter())
        .map(|((method, metadata), handler)| (method.clone(), metadata.clone(), handler.clone()))
        .collect();

    // Generate compile-time router
    let router_impl = crate::compile_time_router::generate_router(
        struct_name,
        &tool_method_data,
        &prompt_method_data,
        &resource_method_data,
        &name_value,
        &version_value,
        &attrs.transports,
    );

    let expanded = quote! {
        #input_impl

        impl #struct_name
        where
            Self: Clone,
        {
            #[doc(hidden)]
            #[allow(non_snake_case)]
            pub fn #metadata_fn_name() -> (&'static str, &'static str, Option<&'static str>) {
                (#name_value, #version_value, #description_value)
            }

            fn create_context_factory() -> turbomcp::ContextFactory {
                use ::turbomcp::{ContextFactory, ContextFactoryConfig, Container};
                let config = ContextFactoryConfig::default();
                let container = Container::new();
                ContextFactory::new(config, container)
            }

            fn discover_tools() -> Vec<(String, String, serde_json::Value)> {
                let mut tools = Vec::new();
                #(
                    {
                        let (name, description, schema) = Self::#tool_metadata_functions();
                        tools.push((
                            name.to_string(),
                            description.to_string(),
                            serde_json::to_value(&schema)
                                .expect("Generated tool schema should always be valid JSON")
                        ));
                    }
                )*
                tools
            }

            /// Returns (name, description, schema) for all registered tools
            pub fn get_tools_metadata() -> Vec<(String, String, serde_json::Value)> {
                Self::discover_tools()
            }

            fn discover_prompts() -> Vec<(String, String, Vec<String>)> {
                let mut prompts = Vec::new();
                #(
                    {
                        let (name, description, _arguments_schema, tags) = Self::#prompt_metadata_functions();
                        prompts.push((
                            name.to_string(),
                            description.to_string(),
                            tags
                        ));
                    }
                )*
                prompts
            }

            /// Returns (name, description, tags) for all registered prompts
            pub fn get_prompts_metadata() -> Vec<(String, String, Vec<String>)> {
                Self::discover_prompts()
            }

            fn discover_resources() -> Vec<(String, String, Vec<String>)> {
                let mut resources = Vec::new();
                #(
                    {
                        let (uri_template, name, _title, _description, _mime_type, tags) = Self::#resource_metadata_functions();
                        resources.push((
                            uri_template.to_string(),
                            name.to_string(),
                            tags
                        ));
                    }
                )*
                resources
            }

            /// Returns (uri, name, tags) for all registered resources
            pub fn get_resources_metadata() -> Vec<(String, String, Vec<String>)> {
                Self::discover_resources()
            }

            /// Create server and get shutdown handle for graceful termination
            pub fn into_server_with_shutdown(self) -> Result<(turbomcp::Server, turbomcp::ShutdownHandle), ::turbomcp::__macro_support::turbomcp_server::McpError> {
                let server = self.create_server()?;
                let shutdown_handle = server.shutdown_handle();
                Ok((server, shutdown_handle))
            }

            /// Get shutdown handle (clones self to create server)
            pub fn shutdown_handle(&self) -> Result<turbomcp::ShutdownHandle, ::turbomcp::__macro_support::turbomcp_server::McpError> {
                let server = self.clone().create_server()?;
                Ok(server.shutdown_handle())
            }

            fn create_server(self) -> Result<turbomcp::Server, ::turbomcp::__macro_support::turbomcp_server::McpError> {
                use ::turbomcp::{RequestContext, ServerBuilder};
                use ::turbomcp::handlers::utils;
                use ::turbomcp::{CallToolRequest, CallToolResult, Content, TextContent};

                let mut builder = ServerBuilder::new()
                    .name(#name_value)
                    .version(#version_value);

                #roots_config
                #protocol_version_config

                let server_instance = self;

                #(
                    {
                        let instance = server_instance.clone();
                        let (tool_name, tool_description, schema) = Self::#tool_metadata_functions();
                        let tool_handler = utils::tool_with_schema(
                            tool_name,
                            tool_description,
                            schema,
                            move |req: CallToolRequest, ctx: RequestContext| {
                                let instance = instance.clone();
                                async move {
                                    instance.#tool_handler_functions(req, ctx).await
                                }
                            }
                        );
                        builder = builder.tool(tool_name, tool_handler)?;
                    }
                )*

                if Self::discover_tools().is_empty() {
                    builder = builder.tool(
                        "example",
                        utils::tool(
                            "example",
                            "Example tool - Add #[tool] methods for auto-registration",
                            |_req: CallToolRequest, _ctx: RequestContext| async move {
                                Ok(CallToolResult {
                                    content: vec![Content::Text(TextContent {
                                        text: "Server running! Add #[tool] methods for automatic registration.".to_string(),
                                        annotations: None,
                                        meta: None,
                                    })],
                                    is_error: None,
                                    structured_content: None,
                                    _meta: None,
                                    task_id: None,
                                })
                            }
                        )
                    )?;
                }

                #(
                    {
                        let instance = server_instance.clone();
                        let (prompt_name, prompt_description, _arguments_schema, _tags) = Self::#prompt_metadata_functions();

                        use ::turbomcp::handlers::utils;
                        use ::turbomcp::__macro_support::turbomcp_protocol::{GetPromptRequest, GetPromptResult};
                        use ::turbomcp::__macro_support::turbomcp_protocol::types::{PromptMessage, Role, Content, TextContent};

                        let prompt_handler = utils::prompt(
                            prompt_name,
                            prompt_description,
                            move |req: GetPromptRequest, ctx: RequestContext| {
                                let instance = instance.clone();
                                async move {
                                    let prompt_content = instance.#prompt_handler_functions(req, ctx).await?;
                                    Ok(GetPromptResult {
                                        description: Some(prompt_description.to_string()),
                                        messages: vec![PromptMessage {
                                            role: Role::User,
                                            content: Content::Text(TextContent {
                                                text: prompt_content,
                                                annotations: None,
                                                meta: None,
                                            }),
                                        }],
                                        _meta: None,
                                    })
                                }
                            }
                        );
                        builder = builder.prompt(prompt_name, prompt_handler)?;
                    }
                )*

                #(
                    {
                        let instance = server_instance.clone();
                        let (resource_uri_template, resource_name, resource_title, resource_description, resource_mime_type, _tags) = Self::#resource_metadata_functions();

                        use ::turbomcp::handlers::FunctionResourceHandler;
                        use ::turbomcp::__macro_support::turbomcp_protocol::{ReadResourceRequest, ReadResourceResult};
                        use ::turbomcp::__macro_support::turbomcp_protocol::types::{ResourceContent, TextResourceContents};

                        let resource_handler = FunctionResourceHandler::new(
                            ::turbomcp::__macro_support::turbomcp_protocol::types::Resource {
                                name: resource_name.to_string(),
                                title: Some(resource_title.to_string()),
                                uri: resource_uri_template.to_string(),
                                description: Some(resource_description.to_string()),
                                mime_type: Some(resource_mime_type.to_string()),
                                annotations: None,
                                size: None,
                                meta: None,
                            },
                            move |req: ReadResourceRequest, ctx: RequestContext| {
                                let instance = instance.clone();
                                async move {
                                    let uri = req.uri.clone();
                                    let resource_content = instance.#resource_handler_functions(req, ctx).await?;
                                    Ok(ReadResourceResult {
                                        contents: vec![ResourceContent::Text(TextResourceContents {
                                            uri,
                                            mime_type: Some("text/plain".to_string()),
                                            text: resource_content,
                                            meta: None,
                                        })],
                                        _meta: None,
                                    })
                                }
                            }
                        );
                        builder = builder.resource(resource_uri_template, resource_handler)?;
                    }
                )*

                Ok(builder.build())
            }

            /// Get server builder for advanced customization
            pub fn builder() -> turbomcp::ServerBuilder {
                turbomcp::ServerBuilder::new()
                    .name(#name_value)
                    .version(#version_value)
            }

            /// Test a tool call directly without full server setup
            pub async fn test_tool_call(
                &self,
                tool_name: &str,
                arguments: serde_json::Value
            ) -> Result<turbomcp::CallToolResult, ::turbomcp::__macro_support::turbomcp_server::McpError> {
                use ::turbomcp::{CallToolRequest, RequestContext};
                use std::collections::HashMap;

                let args_map = if arguments.is_object() {
                    arguments.as_object()
                        .map(|obj| {
                            let mut map = HashMap::new();
                            for (k, v) in obj {
                                map.insert(k.clone(), v.clone());
                            }
                            map
                        })
                } else {
                    None
                };

                let request = CallToolRequest {
                    name: tool_name.to_string(),
                    arguments: args_map,
                    _meta: None,
                    task: None,
                };

                let ctx = RequestContext::new();

                #(
                    if tool_name == stringify!(#tool_methods) {
                        return self.#tool_handler_functions(request, ctx).await;
                    }
                )*

                Err(::turbomcp::__macro_support::turbomcp_server::McpError::internal(format!("Tool '{}' not found", tool_name)))
            }

            /// Returns (name, version, description)
            pub fn server_info() -> (&'static str, &'static str, Option<&'static str>) {
                (#name_value, #version_value, #description_value)
            }

        }

        #router_impl
    };

    TokenStream::from(expanded)
}
