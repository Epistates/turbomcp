//! Context-aware dispatch logic generation
//!
//! This module generates dispatch cases that properly thread RequestContext
//! through to tool/prompt/resource handlers, enabling bidirectional MCP features.

use proc_macro2::TokenStream;
use quote::quote;
use syn::Ident;

/// Generate context-aware tool dispatch cases
///
/// Unlike the original implementation which creates an empty `RequestContext::new()`,
/// this version uses the `ctx` parameter passed from `handle_request_with_context()`.
pub fn generate_context_aware_tool_dispatch(
    tool_methods: &[(Ident, Ident, Ident)],
) -> Vec<TokenStream> {
    tool_methods
        .iter()
        .map(|(method_name, _, handler_fn)| {
            let method_str = method_name.to_string();
            quote! {
                #method_str => {
                    // Parse arguments
                    let args = params
                        .and_then(|p| p.get("arguments"))
                        .and_then(|a| a.as_object())
                        .map(|obj| {
                            let mut map = ::std::collections::HashMap::new();
                            for (k, v) in obj {
                                map.insert(k.clone(), v.clone());
                            }
                            map
                        });

                    let request = ::turbomcp::CallToolRequest {
                        name: tool_name.to_string(),
                        arguments: args,
                        _meta: None,
                        task: None,
                    };

                    // ✅ Use the ctx parameter (with server_to_client populated)
                    // instead of creating empty RequestContext::new()
                    match self.#handler_fn(request, ctx.clone()).await {
                        Ok(result) => {
                            ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcResponse::success(
                                serde_json::json!({
                                    "content": result.content
                                }),
                                req.id.clone()
                            )
                        }
                        // Extract JSON-RPC error code from McpError
                        Err(e) => {
                            ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcResponse::error_response(
                                ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcError {
                                    code: e.jsonrpc_code(),
                                    message: e.to_string(),
                                    data: None,
                                },
                                req.id.clone()
                            )
                        }
                    }
                }
            }
        })
        .collect()
}

/// Generate context-aware prompt dispatch cases
pub fn generate_context_aware_prompt_dispatch(
    prompt_methods: &[(Ident, Ident, Ident)],
) -> Vec<TokenStream> {
    prompt_methods
        .iter()
        .map(|(method_name, _, handler_fn)| {
            let method_str = method_name.to_string();
            quote! {
                #method_str => {
                    // Parse arguments for prompts/get
                    let prompt_args = params
                        .and_then(|p| p.get("arguments"))
                        .and_then(|args| args.as_object())
                        .map(|obj| {
                            let mut map = std::collections::HashMap::new();
                            for (k, v) in obj {
                                map.insert(k.clone(), v.clone());
                            }
                            map
                        });

                    let request = ::turbomcp::__macro_support::turbomcp_protocol::GetPromptRequest {
                        name: #method_str.to_string(),
                        arguments: prompt_args,
                        _meta: None,
                    };

                    // ✅ Use the ctx parameter
                    match self.#handler_fn(request, ctx.clone()).await {
                        Ok(result) => {
                            // Wrap string result in proper MCP GetPromptResult format
                            let get_prompt_result = ::turbomcp::__macro_support::turbomcp_protocol::GetPromptResult {
                                description: None,
                                messages: vec![::turbomcp::__macro_support::turbomcp_protocol::types::PromptMessage {
                                    role: ::turbomcp::__macro_support::turbomcp_protocol::types::Role::User,
                                    content: ::turbomcp::__macro_support::turbomcp_protocol::types::Content::Text(
                                        ::turbomcp::__macro_support::turbomcp_protocol::types::TextContent {
                                            text: result,
                                            annotations: None,
                                            meta: None,
                                        }
                                    ),
                                }],
                                _meta: None,
                            };
                            match serde_json::to_value(&get_prompt_result) {
                                Ok(value) => ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcResponse::success(
                                    value,
                                    req.id.clone()
                                ),
                                Err(e) => ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcResponse::error_response(
                                    ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcError {
                                        code: -32603,
                                        message: format!("Failed to serialize prompt result: {}", e),
                                        data: None,
                                    },
                                    req.id.clone()
                                )
                            }
                        }
                        // Extract JSON-RPC error code from McpError
                        Err(e) => {
                            ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcResponse::error_response(
                                ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcError {
                                    code: e.jsonrpc_code(),
                                    message: e.to_string(),
                                    data: None,
                                },
                                req.id.clone()
                            )
                        }
                    }
                }
            }
        })
        .collect()
}

/// Generate context-aware resource dispatch cases
pub fn generate_context_aware_resource_dispatch(
    resource_methods: &[(Ident, Ident, Ident)],
    uri_match_fn_name: &Ident,
) -> Vec<TokenStream> {
    resource_methods
        .iter()
        .map(|(_method_name, metadata_fn, handler_fn)| {
            let template_string = quote! {
                {
                    let (uri_template, _, _, _, _, _) = Self::#metadata_fn();
                    uri_template
                }
            };

            quote! {
                resource_uri if {
                    let template = #template_string;
                    #uri_match_fn_name(resource_uri, template)
                } => {
                    let request = ::turbomcp::__macro_support::turbomcp_protocol::ReadResourceRequest {
                        uri: resource_uri.to_string(),
                        _meta: None,
                    };

                    // ✅ Use the ctx parameter
                    match self.#handler_fn(request, ctx.clone()).await {
                        Ok(result) => {
                            // Wrap string result in proper MCP ReadResourceResult format
                            let read_resource_result = ::turbomcp::__macro_support::turbomcp_protocol::ReadResourceResult {
                                contents: vec![::turbomcp::__macro_support::turbomcp_protocol::types::ResourceContent::Text(
                                    ::turbomcp::__macro_support::turbomcp_protocol::types::TextResourceContents {
                                        uri: resource_uri.to_string(),
                                        mime_type: Some("text/plain".to_string()),
                                        text: result,
                                        meta: None,
                                    }
                                )],
                                _meta: None,
                            };
                            match serde_json::to_value(&read_resource_result) {
                                Ok(value) => ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcResponse::success(
                                    value,
                                    req.id.clone()
                                ),
                                Err(e) => ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcResponse::error_response(
                                    ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcError {
                                        code: -32603,
                                        message: format!("Failed to serialize resource result: {}", e),
                                        data: None,
                                    },
                                    req.id.clone()
                                )
                            }
                        }
                        // Extract JSON-RPC error code from McpError
                        Err(e) => {
                            ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcResponse::error_response(
                                ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcError {
                                    code: e.jsonrpc_code(),
                                    message: e.to_string(),
                                    data: None,
                                },
                                req.id.clone()
                            )
                        }
                    }
                }
            }
        })
        .collect()
}

/// Generate the handle_request_with_context method
///
/// This is the new internal method that accepts RequestContext as a parameter
/// and properly threads it through to all handlers.
pub fn generate_handle_request_with_context(
    struct_name: &Ident,
    tool_methods: &[(Ident, Ident, Ident)],
    prompt_methods: &[(Ident, Ident, Ident)],
    resource_methods: &[(Ident, Ident, Ident)],
    server_name: &str,
    server_version: &str,
) -> TokenStream {
    // Generate tool/prompt/resource lists (same as original)
    let tool_list_items: Vec<_> = tool_methods
        .iter()
        .map(|(_method_name, metadata_fn, _)| {
            quote! {
                {
                    let (name, description, schema) = Self::#metadata_fn();
                    serde_json::json!({
                        "name": name,
                        "description": description,
                        "inputSchema": serde_json::to_value(&schema)
                            .expect("Generated tool schema should always be valid JSON")
                    })
                }
            }
        })
        .collect();

    let prompt_list_items: Vec<_> = prompt_methods
        .iter()
        .map(|(_method_name, metadata_fn, _)| {
            quote! {
                {
                    let (name, description, arguments_schema, _tags) = Self::#metadata_fn();
                    serde_json::json!({
                        "name": name,
                        "description": description,
                        "arguments": arguments_schema
                    })
                }
            }
        })
        .collect();

    let resource_list_items: Vec<_> = resource_methods
        .iter()
        .map(|(_method_name, metadata_fn, _)| {
            quote! {
                {
                    let (uri_template, name, title, description, mime_type, _tags) = Self::#metadata_fn();
                    serde_json::json!({
                        "uri": uri_template,
                        "name": name,
                        "title": title,
                        "description": description,
                        "mimeType": mime_type
                    })
                }
            }
        })
        .collect();

    // Generate URI match function name
    let uri_match_fn_name = quote::format_ident!(
        "{}_uri_template_matches",
        struct_name.to_string().to_lowercase()
    );

    // Generate context-aware dispatch cases
    let tool_dispatch_cases = generate_context_aware_tool_dispatch(tool_methods);
    let prompt_dispatch_cases = generate_context_aware_prompt_dispatch(prompt_methods);
    let resource_dispatch_cases =
        generate_context_aware_resource_dispatch(resource_methods, &uri_match_fn_name);

    quote! {
        /// Handle a JSON-RPC request with full RequestContext
        ///
        /// This method receives a RequestContext (potentially with server_to_client populated)
        /// and threads it through to all tool/prompt/resource handlers, enabling bidirectional
        /// MCP features like sampling, elicitation, roots, and ping.
        ///
        /// **Internal API**: This method is called by the bidirectional wrapper and transport
        /// layer. It should not be invoked directly by user code.
        async fn handle_request_with_context(
            self: ::std::sync::Arc<Self>,
            req: ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcRequest,
            ctx: ::turbomcp::__macro_support::turbomcp_protocol::RequestContext,
        ) -> ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcResponse {
            use ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::{JsonRpcResponse, JsonRpcError};

            match req.method.as_str() {
                "initialize" => {
                    JsonRpcResponse::success(
                        serde_json::json!({
                            "protocolVersion": ::turbomcp::__macro_support::turbomcp_protocol::PROTOCOL_VERSION,
                            "serverInfo": {
                                "name": #server_name,
                                "version": #server_version
                            },
                            "capabilities": {
                                "tools": {},
                                "prompts": {},
                                "resources": {},
                                "sampling": {},
                                "elicitation": {},
                                "roots": {
                                    "listChanged": false
                                }
                            }
                        }),
                        req.id.clone()
                    )
                }

                "tools/list" => {
                    let tools: Vec<serde_json::Value> = vec![#(#tool_list_items),*];
                    JsonRpcResponse::success(
                        serde_json::json!({
                            "tools": tools
                        }),
                        req.id.clone()
                    )
                }

                "tools/call" => {
                    let params = req.params.as_ref().and_then(|p| p.as_object());
                    let tool_name = params
                        .and_then(|p| p.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("");

                    // Compile-time dispatch to tool handlers
                    match tool_name {
                        #(#tool_dispatch_cases)*
                        _ => {
                            JsonRpcResponse::error_response(
                                JsonRpcError {
                                    code: -32601,
                                    message: format!("Unknown tool: {}", tool_name),
                                    data: None,
                                },
                                req.id.clone()
                            )
                        }
                    }
                }

                "prompts/list" => {
                    let prompts: Vec<serde_json::Value> = vec![#(#prompt_list_items),*];
                    JsonRpcResponse::success(
                        serde_json::json!({
                            "prompts": prompts
                        }),
                        req.id.clone()
                    )
                }

                "prompts/get" => {
                    let params = req.params.as_ref().and_then(|p| p.as_object());
                    let prompt_name = params
                        .and_then(|p| p.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("");

                    match prompt_name {
                        #(#prompt_dispatch_cases)*
                        _ => {
                            JsonRpcResponse::error_response(
                                JsonRpcError {
                                    code: -32601,
                                    message: format!("Unknown prompt: {}", prompt_name),
                                    data: None,
                                },
                                req.id.clone()
                            )
                        }
                    }
                }

                "resources/list" => {
                    let resources: Vec<serde_json::Value> = vec![#(#resource_list_items),*];
                    JsonRpcResponse::success(
                        serde_json::json!({
                            "resources": resources
                        }),
                        req.id.clone()
                    )
                }

                "resources/read" => {
                    let params = req.params.as_ref().and_then(|p| p.as_object());
                    let resource_uri = params
                        .and_then(|p| p.get("uri"))
                        .and_then(|u| u.as_str())
                        .unwrap_or("");

                    match resource_uri {
                        #(#resource_dispatch_cases)*
                        _ => {
                            JsonRpcResponse::error_response(
                                JsonRpcError {
                                    code: -32601,
                                    message: format!("Unknown resource: {}", resource_uri),
                                    data: None,
                                },
                                req.id.clone()
                            )
                        }
                    }
                }

                _ => {
                    JsonRpcResponse::error_response(
                        JsonRpcError {
                            code: -32601,
                            message: format!("Method not found: {}", req.method),
                            data: None,
                        },
                        req.id.clone()
                    )
                }
            }
        }
    }
}
