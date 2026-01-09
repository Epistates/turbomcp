//! Compile-time router generation for TurboMCP
//!
//! This module generates static dispatch routers at compile time,
//! eliminating all lifetime issues and providing maximum performance.
//!
//! **Bidirectional Support**: This module now generates servers with full
//! bidirectional MCP communication support (sampling, elicitation, roots, ping)
//! by creating a wrapper struct with server-to-client capabilities.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Ident;

use crate::bidirectional_wrapper;
use crate::context_aware_dispatch;
use crate::tower_service;

/// Generate compile-time router for HTTP transport
pub fn generate_router(
    struct_name: &Ident,
    tool_methods: &[(Ident, Ident, Ident)], // (method_name, metadata_fn, handler_fn)
    prompt_methods: &[(Ident, Ident, Ident)], // (method_name, metadata_fn, handler_fn)
    resource_methods: &[(Ident, Ident, Ident)], // (method_name, metadata_fn, handler_fn)
    server_name: &str,
    server_version: &str,
    transports: &Option<Vec<String>>,
) -> TokenStream {
    // Generate tool list for tools/list method
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

    // Generate prompt list for prompts/list method
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

    // Generate resource list for resources/list method
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

    // Generate tool dispatch cases
    let tool_dispatch_cases: Vec<_> = tool_methods
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

                    let ctx = ::turbomcp::RequestContext::new();

                    match self.#handler_fn(request, ctx).await {
                        Ok(result) => {
                            ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcResponse::success(
                                serde_json::json!({
                                    "content": result.content
                                }),
                                req.id.clone()
                            )
                        }
                        // FIXED: Extract actual error code from ServerError
                        Err(e) => {
                            ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcResponse::error_response(
                                ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcError {
                                    code: e.error_code(),
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
        .collect();

    // Generate prompt dispatch cases for prompts/get
    let prompt_dispatch_cases: Vec<_> = prompt_methods
        .iter()
        .map(|(method_name, _, handler_fn)| {
            let method_str = method_name.to_string();
            quote! {
                #method_str => {
                    // Parse arguments for prompts/get - convert to HashMap<String, Value>
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

                    let ctx = ::turbomcp::RequestContext::new();

                    match self.#handler_fn(request, ctx).await {
                        Ok(result) => {
                            // Wrap string result in proper MCP GetPromptResult format
                            let get_prompt_result = ::turbomcp::__macro_support::turbomcp_protocol::GetPromptResult {
                                description: None,
                                messages: vec![::turbomcp::__macro_support::turbomcp_protocol::types::PromptMessage {
                                    role: ::turbomcp::__macro_support::turbomcp_protocol::types::Role::User,
                                    content: ::turbomcp::__macro_support::turbomcp_protocol::types::Content::Text(::turbomcp::__macro_support::turbomcp_protocol::types::TextContent {
                                        text: result,
                                        annotations: None,
                                        meta: None,
                                    }),
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
                        // FIXED: Extract actual error code from ServerError
                        Err(e) => {
                            ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcResponse::error_response(
                                ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcError {
                                    code: e.error_code(),
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
        .collect();

    // Generate unique function name for URI template matching to avoid conflicts
    let uri_match_fn_name = format_ident!(
        "{}_uri_template_matches",
        struct_name.to_string().to_lowercase()
    );

    // Generate resource dispatch cases for resources/read with compile-time URI template matching
    let resource_dispatch_cases: Vec<_> = resource_methods
        .iter()
        .map(|(_method_name, metadata_fn, handler_fn)| {
            // Get the URI template at compile time for code generation
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

                    let ctx = ::turbomcp::RequestContext::new();

                    match self.#handler_fn(request, ctx).await {
                        Ok(result) => {
                            // Wrap string result in proper MCP ReadResourceResult format
                            let read_resource_result = ::turbomcp::__macro_support::turbomcp_protocol::ReadResourceResult {
                                contents: vec![::turbomcp::__macro_support::turbomcp_protocol::types::ResourceContent::Text(::turbomcp::__macro_support::turbomcp_protocol::types::TextResourceContents {
                                    uri: resource_uri.to_string(),
                                    mime_type: Some("text/plain".to_string()),
                                    text: result,
                                    meta: None,
                                })],
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
                        // FIXED: Extract actual error code from ServerError
                        Err(e) => {
                            ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcResponse::error_response(
                                ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcError {
                                    code: e.error_code(),
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
        .collect();

    // ===================================================================
    // Bidirectional Wrapper Generation - Enables Server-to-Client Requests
    // ===================================================================

    // Generate the bidirectional wrapper struct
    let wrapper_code = bidirectional_wrapper::generate_bidirectional_wrapper(
        struct_name,
        server_name,
        server_version,
    );

    // Generate context-aware request handler (replaces empty RequestContext::new())
    let context_handler = context_aware_dispatch::generate_handle_request_with_context(
        struct_name,
        tool_methods,
        prompt_methods,
        resource_methods,
        server_name,
        server_version,
    );

    // Generate bidirectional transport methods (run_stdio, run_http, run_websocket)
    // Pass transports filter to only generate specified transports
    let bidirectional_transports =
        bidirectional_wrapper::generate_bidirectional_transport_methods(struct_name, transports);

    // ===================================================================
    // Tower Service Generation - Composable Middleware Support
    // ===================================================================

    // Generate Tower Layer and Service implementations
    let tower_code =
        tower_service::generate_tower_service(struct_name, server_name, server_version);

    quote! {
        // Helper function for URI template matching - generated at compile time for maximum performance
        fn #uri_match_fn_name(uri: &str, template: &str) -> bool {
            // Simple implementation for literal templates
            if !template.contains('{') {
                return uri == template;
            }

            // Parse template for variable extraction with high performance
            let template_parts: Vec<&str> = template.split('/').filter(|s| !s.is_empty()).collect();
            let uri_parts: Vec<&str> = uri.split('/').filter(|s| !s.is_empty()).collect();

            if template_parts.len() != uri_parts.len() {
                return false;
            }

            for (template_part, uri_part) in template_parts.iter().zip(uri_parts.iter()) {
                if template_part.starts_with('{') && template_part.ends_with('}') {
                    // Variable part - matches any non-empty string
                    if uri_part.is_empty() {
                        return false;
                    }
                } else if template_part != uri_part {
                    // Literal part that doesn't match
                    return false;
                }
            }

            true
        }

        // ===================================================================
        // JsonRpcHandler Implementation - Transport-Agnostic Request Handling
        // ===================================================================

        #[::turbomcp::async_trait]
        impl ::turbomcp::__macro_support::turbomcp_protocol::JsonRpcHandler for #struct_name
        where
            Self: Clone + Send + Sync + 'static,
        {
            async fn handle_request(
                &self,
                req_value: serde_json::Value,
            ) -> serde_json::Value {
                use ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcResponse, JsonRpcError};

                // Parse the request
                let req: JsonRpcRequest = match serde_json::from_value(req_value) {
                    Ok(r) => r,
                    Err(e) => {
                        return serde_json::json!({
                            "jsonrpc": "2.0",
                            "error": {
                                "code": -32700,
                                "message": format!("Parse error: {}", e)
                            },
                            "id": null
                        });
                    }
                };

                let response = match req.method.as_str() {
                    "initialize" => {
                        JsonRpcResponse::success(
                            serde_json::json!({
                                "protocolVersion": "2025-06-18",
                                "serverInfo": {
                                    "name": #server_name,
                                    "version": #server_version
                                },
                                "capabilities": {
                                    "tools": {},
                                    "resources": {},
                                    "prompts": {},
                                    "elicitation": {}
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
                            .and_then(|n| n.as_str());

                        match tool_name {
                            Some(tool_name) => {
                                match tool_name {
                                    #(#tool_dispatch_cases)*
                                    _ => {
                                        JsonRpcResponse::error_response(
                                            JsonRpcError {
                                                code: -32602,
                                                message: format!("Unknown tool: {}", tool_name),
                                                data: None,
                                            },
                                            req.id.clone()
                                        )
                                    }
                                }
                            }
                            None => {
                                JsonRpcResponse::error_response(
                                    JsonRpcError {
                                        code: -32602,
                                        message: "Missing tool name".to_string(),
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
                            .and_then(|n| n.as_str());
                        match prompt_name {
                            Some(prompt_name) => {
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
                            None => {
                                JsonRpcResponse::error_response(
                                    JsonRpcError {
                                        code: -32602,
                                        message: "Missing prompt name".to_string(),
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
                            .and_then(|u| u.as_str());
                        match resource_uri {
                            Some(resource_uri) => {
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
                            None => {
                                JsonRpcResponse::error_response(
                                    JsonRpcError {
                                        code: -32602,
                                        message: "Missing resource URI".to_string(),
                                        data: None,
                                    },
                                    req.id.clone()
                                )
                            }
                        }
                    }

                    "elicitation/create" => {
                        // Handle elicitation responses from client
                        JsonRpcResponse::success(
                            serde_json::json!({}),
                            req.id.clone()
                        )
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
                };

                // Convert response to JSON
                serde_json::to_value(response).unwrap_or_else(|e| {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32603,
                            "message": format!("Internal error: {}", e)
                        },
                        "id": null
                    })
                })
            }

            fn server_info(&self) -> ::turbomcp::__macro_support::turbomcp_protocol::ServerInfo {
                ::turbomcp::__macro_support::turbomcp_protocol::ServerInfo {
                    name: #server_name.to_string(),
                    version: #server_version.to_string(),
                }
            }

            fn capabilities(&self) -> serde_json::Value {
                serde_json::json!({
                    "tools": {},
                    "resources": {},
                    "prompts": {},
                    "elicitation": {}
                })
            }
        }

        // ===================================================================
        // Convenience Methods - Transport-Specific Helpers
        // ===================================================================

        impl #struct_name
        where
            Self: Clone + Send + Sync + 'static,
        {
            // NOTE: into_mcp_router methods have been removed to avoid compilation
            // issues with feature-gated types. Use run_http() instead.


            // ===================================================================
            // NOTE: Transport methods (run_stdio, run_http, run_websocket) are now
            // generated by bidirectional_wrapper module to support server-to-client
            // requests (sampling, elicitation, roots, ping).
            // See bidirectional_transports code generation below.
            // ===================================================================

            /// Handle a single JSON-RPC request with compile-time dispatch
            async fn handle_request(
                self: ::std::sync::Arc<Self>,
                req: ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcRequest
            ) -> ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcResponse {
                use ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::{JsonRpcResponse, JsonRpcVersion, JsonRpcError};

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
                                    "sampling": {}
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
                        // Compile-time dispatch to prompt handlers
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
                        // Compile-time dispatch to resource handlers - match by URI pattern
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

        // ===================================================================
        // Context-Aware Request Handler - Enables Bidirectional MCP
        // ===================================================================

        impl #struct_name
        where
            Self: Clone + Send + Sync + 'static,
        {
            #context_handler
        }

        // ===================================================================
        // Bidirectional Wrapper - Server-to-Client Capabilities
        // ===================================================================

        #wrapper_code

        // ===================================================================
        // Bidirectional Transport Methods - run_stdio(), run_http(), etc.
        // ===================================================================

        #bidirectional_transports

        // ===================================================================
        // Tower Integration - Composable Middleware Support
        // ===================================================================

        #tower_code
    }
}
