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

/// Generate compile-time router for HTTP transport
pub fn generate_router(
    struct_name: &Ident,
    tool_methods: &[(Ident, Ident, Ident)], // (method_name, metadata_fn, handler_fn)
    prompt_methods: &[(Ident, Ident, Ident)], // (method_name, metadata_fn, handler_fn)
    resource_methods: &[(Ident, Ident, Ident)], // (method_name, metadata_fn, handler_fn)
    server_name: &str,
    server_version: &str,
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
                    };

                    let ctx = ::turbomcp::RequestContext::new();

                    match self.#handler_fn(request, ctx).await {
                        Ok(result) => {
                            ::turbomcp::turbomcp_protocol::jsonrpc::JsonRpcResponse::success(
                                serde_json::json!({
                                    "content": result.content
                                }),
                                req.id.clone()
                            )
                        }
                        Err(e) => {
                            ::turbomcp::turbomcp_protocol::jsonrpc::JsonRpcResponse::error_response(
                                ::turbomcp::turbomcp_protocol::jsonrpc::JsonRpcError {
                                    code: -32603,
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

                    let request = ::turbomcp::turbomcp_protocol::GetPromptRequest {
                        name: #method_str.to_string(),
                        arguments: prompt_args,
                        _meta: None,
                    };

                    let ctx = ::turbomcp::RequestContext::new();

                    match self.#handler_fn(request, ctx).await {
                        Ok(result) => {
                            // Wrap string result in proper MCP GetPromptResult format
                            let get_prompt_result = ::turbomcp::turbomcp_protocol::GetPromptResult {
                                description: None,
                                messages: vec![::turbomcp::turbomcp_protocol::types::PromptMessage {
                                    role: ::turbomcp::turbomcp_protocol::types::Role::User,
                                    content: ::turbomcp::turbomcp_protocol::types::Content::Text(::turbomcp::turbomcp_protocol::types::TextContent {
                                        text: result,
                                        annotations: None,
                                        meta: None,
                                    }),
                                }],
                                _meta: None,
                            };
                            match serde_json::to_value(&get_prompt_result) {
                                Ok(value) => ::turbomcp::turbomcp_protocol::jsonrpc::JsonRpcResponse::success(
                                    value,
                                    req.id.clone()
                                ),
                                Err(e) => ::turbomcp::turbomcp_protocol::jsonrpc::JsonRpcResponse::error_response(
                                    ::turbomcp::turbomcp_protocol::jsonrpc::JsonRpcError {
                                        code: -32603,
                                        message: format!("Failed to serialize prompt result: {}", e),
                                        data: None,
                                    },
                                    req.id.clone()
                                )
                            }
                        }
                        Err(e) => {
                            ::turbomcp::turbomcp_protocol::jsonrpc::JsonRpcResponse::error_response(
                                ::turbomcp::turbomcp_protocol::jsonrpc::JsonRpcError {
                                    code: -32603,
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
                    let request = ::turbomcp::turbomcp_protocol::ReadResourceRequest {
                        uri: resource_uri.to_string(),
                        _meta: None,
                    };

                    let ctx = ::turbomcp::RequestContext::new();

                    match self.#handler_fn(request, ctx).await {
                        Ok(result) => {
                            // Wrap string result in proper MCP ReadResourceResult format
                            let read_resource_result = ::turbomcp::turbomcp_protocol::ReadResourceResult {
                                contents: vec![::turbomcp::turbomcp_protocol::types::ResourceContent::Text(::turbomcp::turbomcp_protocol::types::TextResourceContents {
                                    uri: resource_uri.to_string(),
                                    mime_type: Some("text/plain".to_string()),
                                    text: result,
                                    meta: None,
                                })],
                                _meta: None,
                            };
                            match serde_json::to_value(&read_resource_result) {
                                Ok(value) => ::turbomcp::turbomcp_protocol::jsonrpc::JsonRpcResponse::success(
                                    value,
                                    req.id.clone()
                                ),
                                Err(e) => ::turbomcp::turbomcp_protocol::jsonrpc::JsonRpcResponse::error_response(
                                    ::turbomcp::turbomcp_protocol::jsonrpc::JsonRpcError {
                                        code: -32603,
                                        message: format!("Failed to serialize resource result: {}", e),
                                        data: None,
                                    },
                                    req.id.clone()
                                )
                            }
                        }
                        Err(e) => {
                            ::turbomcp::turbomcp_protocol::jsonrpc::JsonRpcResponse::error_response(
                                ::turbomcp::turbomcp_protocol::jsonrpc::JsonRpcError {
                                    code: -32603,
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
    let bidirectional_transports =
        bidirectional_wrapper::generate_bidirectional_transport_methods(struct_name);

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
        impl ::turbomcp::turbomcp_protocol::JsonRpcHandler for #struct_name
        where
            Self: Clone + Send + Sync + 'static,
        {
            async fn handle_request(
                &self,
                req_value: serde_json::Value,
            ) -> serde_json::Value {
                use ::turbomcp::turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcResponse, JsonRpcError};

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

            fn server_info(&self) -> ::turbomcp::turbomcp_protocol::ServerInfo {
                ::turbomcp::turbomcp_protocol::ServerInfo {
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
            /// Create MCP 2025-06-18 compliant Axum router with default "/mcp" endpoint
            ///
            /// Returns an Axum router that can be composed into a larger application.
            /// This method is for advanced users who want to integrate MCP into an existing
            /// Axum application with custom middleware, routing, or configuration.
            ///
            /// **Full MCP compliance:**
            /// - ✅ GET support for SSE streams
            /// - ✅ POST support for JSON-RPC messages
            /// - ✅ DELETE support for session cleanup
            /// - ✅ Session management and message replay
            /// - ✅ Smart localhost security (Origin validation)
            ///
            /// # Example
            /// ```rust,no_run
            /// use axum::Router;
            /// use std::sync::Arc;
            ///
            /// let mcp_server = MyServer::new();
            /// let mcp_router = Arc::new(mcp_server).into_mcp_router();
            ///
            /// // Compose into larger application
            /// let app = Router::new()
            ///     .merge(mcp_router)
            ///     .route("/health", get(health_check));
            /// ```
            ///
            /// **Requires**: Enable the `http` feature in your `turbomcp` dependency:
            ///
            /// ```toml
            /// [dependencies]
            /// turbomcp = { version = "2.0", features = ["http"] }
            /// ```
            ///
            /// For standalone servers, use `run_http()` instead.
            pub fn into_mcp_router(self: ::std::sync::Arc<Self>) -> ::turbomcp::axum::Router {
                self.into_mcp_router_with_path("/mcp")
            }

            /// Create MCP 2025-06-18 compliant Axum router with custom endpoint path
            ///
            /// Returns an Axum router that can be composed into a larger application.
            /// This method is for advanced users who want to integrate MCP into an existing
            /// Axum application with custom middleware, routing, or configuration.
            ///
            /// **Full MCP compliance:**
            /// - ✅ GET support for SSE streams
            /// - ✅ POST support for JSON-RPC messages
            /// - ✅ DELETE support for session cleanup
            /// - ✅ Session management and message replay
            /// - ✅ Smart localhost security (Origin validation)
            ///
            /// # Arguments
            ///
            /// * `path` - Custom endpoint path (e.g., "/api/mcp", "/v1/mcp")
            ///
            /// # Example
            /// ```rust,no_run
            /// use axum::Router;
            /// use std::sync::Arc;
            ///
            /// let mcp_server = MyServer::new();
            /// let mcp_router = Arc::new(mcp_server).into_mcp_router_with_path("/api/v1/mcp");
            ///
            /// // Compose into larger application
            /// let app = Router::new()
            ///     .merge(mcp_router)
            ///     .route("/api/health", get(health_check));
            /// ```
            ///
            /// **Requires**: Enable the `http` feature in your `turbomcp` dependency:
            ///
            /// ```toml
            /// [dependencies]
            /// turbomcp = { version = "2.0", features = ["http"] }
            /// ```
            ///
            /// For standalone servers, use `run_http_with_path()` instead.
            pub fn into_mcp_router_with_path(self: ::std::sync::Arc<Self>, path: &str) -> ::turbomcp::axum::Router {
                use ::std::sync::Arc;
                use ::turbomcp::turbomcp_transport::streamable_http_v2::{create_router, StreamableHttpConfig};

                // Create MCP 2025-06-18 compliant transport configuration
                // Default security config with smart localhost handling:
                // - Validates Origin headers when present
                // - Allows localhost→localhost without Origin (Claude Code compatibility)
                // - Blocks remote clients without valid Origin (DNS rebinding protection)
                let config = StreamableHttpConfig {
                    bind_addr: "0.0.0.0:0".to_string(), // Not used for router creation
                    endpoint_path: path.to_string(),
                    ..Default::default()
                };

                // Create router with full MCP compliance (GET/POST/DELETE)
                create_router(config, self)
            }

            // ===================================================================
            // NOTE: Transport methods (run_stdio, run_http, run_websocket) are now
            // generated by bidirectional_wrapper module to support server-to-client
            // requests (sampling, elicitation, roots, ping).
            // See bidirectional_transports code generation below.
            // ===================================================================

            /// Handle a single JSON-RPC request with compile-time dispatch
            async fn handle_request(
                self: ::std::sync::Arc<Self>,
                req: ::turbomcp::turbomcp_protocol::jsonrpc::JsonRpcRequest
            ) -> ::turbomcp::turbomcp_protocol::jsonrpc::JsonRpcResponse {
                use ::turbomcp::turbomcp_protocol::jsonrpc::{JsonRpcResponse, JsonRpcVersion, JsonRpcError};

                match req.method.as_str() {
                    "initialize" => {
                        JsonRpcResponse::success(
                            serde_json::json!({
                                "protocolVersion": ::turbomcp::turbomcp_protocol::PROTOCOL_VERSION,
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
    }
}
