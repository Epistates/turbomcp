//! TurboMCP server macro implementation
//!
//! This macro provides ergonomic server creation by:
//! - Automatically discovering and registering #[tool], #[resource], and #[prompt] methods
//! - Generating a complete MCP server with zero boilerplate
//! - Integrating seamlessly with the existing builder pattern for advanced use cases
//! - Providing proper JSON schemas and Context injection
//! - Creating the essential run_stdio() method

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{Ident, ItemImpl};

/// Generate the TurboMCP server implementation (idiomatic impl block pattern)
pub fn generate_server_impl(args: TokenStream, input_impl: ItemImpl) -> TokenStream {
    // Parse server attributes using world-class syn parsing
    let attrs = match crate::attrs::ServerAttrs::from_args(args) {
        Ok(attrs) => attrs,
        Err(e) => return e.to_compile_error().into(),
    };
    // Extract the struct name from the impl block
    let struct_name = match &*input_impl.self_ty {
        syn::Type::Path(type_path) => &type_path.path.segments.last().unwrap().ident,
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
    );

    // Idiomatic implementation for impl blocks only
    let expanded = quote! {
        #input_impl

        impl #struct_name
        where
            Self: Clone,
        {
            /// Get server metadata (generated by macro)
            #[doc(hidden)]
            #[allow(non_snake_case)]
            pub fn #metadata_fn_name() -> (&'static str, &'static str, Option<&'static str>) {
                (#name_value, #version_value, #description_value)
            }

            /// Initialize context factory for this server
            fn create_context_factory() -> turbomcp::ContextFactory {
                use turbomcp::{ContextFactory, ContextFactoryConfig, Container};

                let config = ContextFactoryConfig::default();
                let container = Container::new();
                ContextFactory::new(config, container)
            }

            /// Tool discovery - collects all #[tool] methods
            fn discover_tools() -> Vec<(String, String, serde_json::Value)> {
                let mut tools = Vec::new();

                // Auto-discovered tools from #[tool] methods with real schemas
                #(
                    {
                        let (name, description, schema) = Self::#tool_metadata_functions();
                        tools.push((
                            name.to_string(),
                            description.to_string(),
                            schema
                        ));
                    }
                )*

                tools
            }

            /// Get all tools metadata for testing and validation
            ///
            /// Returns a vector of (name, description, schema) tuples for all registered tools.
            /// This is essential for integration testing and validating schema generation.
            pub fn get_tools_metadata() -> Vec<(String, String, serde_json::Value)> {
                Self::discover_tools()
            }

            /// Prompt discovery - collects all #[prompt] methods
            fn discover_prompts() -> Vec<(String, String, Vec<String>)> {
                let mut prompts = Vec::new();

                // Auto-discovered prompts from #[prompt] methods
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

            /// Get all prompts metadata for testing and validation
            ///
            /// Returns a vector of (name, description, tags) tuples for all registered prompts.
            /// This is essential for integration testing and validating prompt discovery.
            pub fn get_prompts_metadata() -> Vec<(String, String, Vec<String>)> {
                Self::discover_prompts()
            }

            /// Resource discovery - collects all #[resource] methods
            fn discover_resources() -> Vec<(String, String, Vec<String>)> {
                let mut resources = Vec::new();

                // Auto-discovered resources from #[resource] methods
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

            /// Get all resources metadata for testing and validation
            ///
            /// Returns a vector of (name, description, tags) tuples for all registered resources.
            /// This is essential for integration testing and validating resource discovery.
            pub fn get_resources_metadata() -> Vec<(String, String, Vec<String>)> {
                Self::discover_resources()
            }

            /// Create server and get shutdown handle for graceful termination
            ///
            /// Essential for production deployments, container orchestration, and coordinated
            /// system shutdown. Returns both the server and a handle for external control.
            pub fn into_server_with_shutdown(self) -> Result<(turbomcp::Server, turbomcp::ShutdownHandle), turbomcp::ServerError> {
                let server = self.create_server()?;
                let shutdown_handle = server.shutdown_handle();
                Ok((server, shutdown_handle))
            }

            /// Get a shutdown handle for graceful server termination (legacy method)
            ///
            /// Essential for production deployments, container orchestration, and coordinated
            /// system shutdown. Enables external control over server lifecycle.
            pub fn shutdown_handle(&self) -> Result<turbomcp::ShutdownHandle, turbomcp::ServerError> {
                // For compatibility, we clone self to create the server
                // This works because the actual tool implementations will be captured properly
                let server = self.clone().create_server()?;
                Ok(server.shutdown_handle())
            }

            // Transport methods (run_stdio, run_http, etc.) are generated by compile_time_router
            // to avoid lifetime issues and provide maximum performance through static dispatch

            /// Create and configure the underlying server instance
            fn create_server(self) -> Result<turbomcp::Server, turbomcp::ServerError> {
                use turbomcp::{RequestContext, ServerBuilder};
                use turbomcp::handlers::utils;
                use turbomcp::{CallToolRequest, CallToolResult, Content, TextContent};

                // Create server builder with metadata from macro
                let mut builder = ServerBuilder::new()
                    .name(#name_value)
                    .version(#version_value);

                // Configure roots if specified in macro
                #roots_config

                // Tool auto-discovery and registration with actual method calls
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
                                    // Call the actual generated handler method
                                    instance.#tool_handler_functions(req, ctx).await
                                }
                            }
                        );
                        builder = builder.tool(tool_name, tool_handler)?;
                    }
                )*

                // If no tools discovered, provide helpful example
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
                                })
                            }
                        )
                    )?;
                }

                // Prompt auto-discovery and registration
                #(
                    {
                        let instance = server_instance.clone();
                        let (prompt_name, prompt_description, _arguments_schema, _tags) = Self::#prompt_metadata_functions();

                        // Create prompt handler using utils helper
                        use turbomcp::handlers::utils;
                        use turbomcp_protocol::{GetPromptRequest, GetPromptResult};
                        use turbomcp_protocol::types::{PromptMessage, Role, Content, TextContent};

                        let prompt_handler = utils::prompt(
                            prompt_name,
                            prompt_description,
                            move |req: GetPromptRequest, ctx: RequestContext| {
                                let instance = instance.clone();
                                async move {
                                    // Call the actual generated handler method (returns String)
                                    let prompt_content = instance.#prompt_handler_functions(req, ctx).await?;

                                    // Convert string result to proper MCP prompt format
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
                                    })
                                }
                            }
                        );
                        builder = builder.prompt(prompt_name, prompt_handler)?;
                    }
                )*

                // Resource auto-discovery and registration
                #(
                    {
                        let instance = server_instance.clone();
                        let (resource_uri_template, resource_name, resource_title, resource_description, resource_mime_type, _tags) = Self::#resource_metadata_functions();

                        // Create resource handler using the FunctionResourceHandler
                        use turbomcp::handlers::FunctionResourceHandler;
                        use turbomcp_protocol::{ReadResourceRequest, ReadResourceResult};
                        use turbomcp_protocol::types::{ResourceContent, TextResourceContents};

                        let resource_handler = FunctionResourceHandler::new(
                            turbomcp_protocol::types::Resource {
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
                                    // Extract URI before moving req
                                    let uri = req.uri.clone();

                                    // Call the actual generated handler method
                                    let resource_content = instance.#resource_handler_functions(req, ctx).await?;

                                    // Convert string result to proper MCP resource format
                                    Ok(ReadResourceResult {
                                        contents: vec![ResourceContent::Text(TextResourceContents {
                                            uri,
                                            mime_type: Some("text/plain".to_string()),
                                            text: resource_content,
                                            meta: None,
                                        })],
                                    })
                                }
                            }
                        );
                        builder = builder.resource(resource_name, resource_handler)?;
                    }
                )*

                Ok(builder.build())
            }

            /// Get server builder for advanced use cases
            pub fn builder() -> turbomcp::ServerBuilder {
                turbomcp::ServerBuilder::new()
                    .name(#name_value)
                    .version(#version_value)
            }

            /// Test a tool call directly for testing
            ///
            /// This function enables direct testing of tool handlers without requiring
            /// full server initialization or transport layer setup.
            pub async fn test_tool_call(
                &self,
                tool_name: &str,
                arguments: serde_json::Value
            ) -> Result<turbomcp::CallToolResult, turbomcp::ServerError> {
                use turbomcp::{CallToolRequest, RequestContext};
                use std::collections::HashMap;

                // Convert JSON arguments to HashMap<String, Value>
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
                };

                let ctx = RequestContext::new();

                // Find and call the appropriate handler
                #(
                    if tool_name == stringify!(#tool_methods) {
                        return self.#tool_handler_functions(request, ctx).await;
                    }
                )*

                Err(turbomcp::ServerError::handler(format!("Tool '{}' not found", tool_name)))
            }

            /// Get server information (for integration with other systems)
            pub fn server_info() -> (&'static str, &'static str, Option<&'static str>) {
                (#name_value, #version_value, #description_value)
            }

        }

        // Add compile-time router implementation
        #router_impl
    };

    TokenStream::from(expanded)
}
